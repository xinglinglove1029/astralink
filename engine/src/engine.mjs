import http from 'node:http';
import { isIP } from 'node:net';
import { randomUUID, createHash, timingSafeEqual } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { isInitializeRequest } from '@modelcontextprotocol/sdk/types.js';
import { AuthQueue } from './auth.mjs';
import { Diagnostics } from './diagnostics.mjs';
import { ConnectionPool } from './pool.mjs';
import { validateProfiles, UUID } from './profiles.mjs';
import { registerUpstreamTools, withMcpActor } from './upstream.mjs';
import { recordToolCall } from './audit.mjs';
import { EngineError } from './errors.mjs';
import { tokenMatches, validOrigin, loopbackAuthority } from './security.mjs';
const MAX_BODY = 1024 * 1024;
function json(response, status, body) {
  if (response.headersSent || response.destroyed)
    return;
  response.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' });
  response.end(JSON.stringify(body));
}
async function readJson(request) {
  if (!/^application\/json(?:\s*;|$)/i.test(request.headers['content-type'] ?? ''))
    throw new EngineError('CONTENT_TYPE', '需要 JSON 请求', 415);
  if (Number(request.headers['content-length']) > MAX_BODY) {
    request.resume();
    throw new EngineError('BODY_TOO_LARGE', '请求过大', 413);
  }
  const chunks = [];
  let size = 0;
  for await (const chunk of request) {
    size += chunk.length;
    if (size > MAX_BODY)
      throw new EngineError('BODY_TOO_LARGE', '请求过大', 413);
    chunks.push(chunk);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString('utf8'));
  }
  catch {
    throw new EngineError('INVALID_JSON', 'JSON 无效');
  }
}
export async function createEngine({ port = 18766, controlToken, mcpToken, identitiesFile, auditFile, remoteHost, authOptions, poolOptions, maxSessions = 64, sessionTtlMs = 1800000 } = {}) {
  if (!Number.isInteger(port) || port < 0 || port > 65535)
    throw new EngineError('INVALID_PORT', '端口无效');
  if (![controlToken, mcpToken].every(token => typeof token === 'string' && token.length > 0 && token.length <= 4096 && !/\s/.test(token)) || controlToken === mcpToken)
    throw new EngineError('INVALID_TOKENS', '需要两个独立访问令牌');
  if (!Number.isSafeInteger(maxSessions) || maxSessions < 1 || !Number.isSafeInteger(sessionTtlMs) || sessionTtlMs < 1)
    throw new EngineError('INVALID_SESSION_LIMIT', '会话限制无效');
  if (remoteHost && (!isIP(remoteHost) || !identitiesFile || !auditFile))
    throw new EngineError('INVALID_REMOTE_HOST', '远程 MCP 需要合法 IP 和用户认证配置');
  let remoteAuthority = remoteHost && `${remoteHost.includes(':') ? `[${remoteHost}]` : remoteHost}:${port}`;
  const diagnostics = new Diagnostics();
  const auth = new AuthQueue(authOptions), pool = new ConnectionPool(auth, { ...poolOptions, diagnostics }), sessions = new Map();
  let stopping = false, closePromise, updates = Promise.resolve(), pendingSessions = 0;
  let resolveClosed;
  const closed = new Promise(resolve => {
    resolveClosed = resolve;
  });
  async function closeSession(id, entry) {
    sessions.delete(id);
    try {
      await entry.mcp.close();
    }
    catch { /* No exception text in diagnostics. */
    }
  }
  const timer = setInterval(() => {
    for (const [id, entry] of sessions)
      if (!entry.active && Date.now() - entry.touched > sessionTtlMs)
        void closeSession(id, entry);
  }, Math.min(sessionTtlMs, 30000));
  timer.unref();
  const server = http.createServer(async (request, response) => {
    try {
      if (stopping) {
        json(response, 503, { error: '服务正在停止' });
        return;
      }
      const remote = request.socket.remoteAddress;
      const localRequest = ['127.0.0.1', '::ffff:127.0.0.1'].includes(remote)
        && loopbackAuthority(request.headers.host ?? '') && validOrigin(request.headers.origin);
      const remoteMcp = remoteHost && request.url?.split('?')[0] === '/mcp'
        && (request.headers.host === remoteAuthority || loopbackAuthority(request.headers.host ?? ''))
        && (request.headers.origin === undefined || validOrigin(request.headers.origin)
          || ['http', 'https'].some(scheme => request.headers.origin === `${scheme}://${remoteAuthority}`));
      if (!localRequest && !remoteMcp) {
        json(response, 403, { error: '仅允许本机请求' });
        return;
      }
      const url = new URL(request.url, 'http://127.0.0.1');
      const mcp = url.pathname === '/mcp';
      if (!mcp && !url.pathname.startsWith('/control/')) {
        json(response, 404, { error: '接口不存在' });
        return;
      }
      let actor;
      if (mcp && identitiesFile) {
        try {
          const bearer = request.headers.authorization?.match(/^Bearer (\S+)$/)?.[1];
          const hash = bearer && createHash('sha256').update(bearer).digest();
          const users = JSON.parse(readFileSync(identitiesFile, { encoding: 'utf8' }));
          if (hash && Array.isArray(users) && users.length <= 64)
            actor = users.find(user => typeof user?.hash === 'string' && /^[a-f0-9]{64}$/.test(user.hash)
              && timingSafeEqual(hash, Buffer.from(user.hash, 'hex')) && ['admin', 'operator'].includes(user.role));
        } catch { /* Missing or invalid identity data fails closed. */ }
      }
      if ((mcp && identitiesFile && !actor) || (!mcp || !identitiesFile) && !tokenMatches(request.headers.authorization, mcp ? mcpToken : controlToken)) {
        json(response, 401, { error: '访问令牌无效' });
        return;
      }
      if (mcp) {
        if (!['GET', 'POST', 'DELETE'].includes(request.method)) {
          json(response, 405, { error: '请求方法无效' });
          return;
        }
        const id = request.headers['mcp-session-id'];
        let entry = id ? sessions.get(id) : undefined;
        if (id && !entry) {
          json(response, 404, { error: '会话不存在', code: 'SESSION_NOT_FOUND' });
          return;
        }
        if (entry && identitiesFile && entry.userId !== actor.id) {
          json(response, 403, { error: '会话与用户不匹配' });
          return;
        }
        const body = request.method === 'POST' ? await readJson(request) : undefined;
        if (!entry) {
          if (request.method !== 'POST' || !isInitializeRequest(body)) {
            json(response, 400, { error: '请先初始化 MCP 会话' });
            return;
          }
          if (sessions.size + pendingSessions >= maxSessions) {
            json(response, 429, { error: '会话数量已达上限' });
            return;
          }
          pendingSessions++;
          const mcpServer = new McpServer({ name: 'astralink', version: '0.1.0' });
          entry = { mcp: mcpServer, active: 0, touched: Date.now(), userId: actor?.id };
          const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: randomUUID, enableJsonResponse: true, onsessioninitialized: sessionId => sessions.set(sessionId, entry) });
          entry.transport = transport;
          registerUpstreamTools(mcpServer, pool, (event) => recordToolCall(auditFile, event));
          try {
            await mcpServer.connect(transport);
          }
          catch (error) {
            pendingSessions--;
            await mcpServer.close();
            throw error;
          }
          const sdkOnClose = transport.onclose;
          transport.onclose = () => {
            sdkOnClose?.();
            if (transport.sessionId)
              sessions.delete(transport.sessionId);
          };
          try {
            entry.active++;
            await withMcpActor(actor, () => transport.handleRequest(request, response, body));
          }
          finally {
            entry.active--;
            entry.touched = Date.now();
            pendingSessions--;
            if (!transport.sessionId)
              await mcpServer.close();
          }
        }
        else {
          const counted = request.method === 'POST';
          if (counted)
            entry.active++;
          entry.touched = Date.now();
          try {
            await withMcpActor(actor, () => entry.transport.handleRequest(request, response, body));
          }
          finally {
            if (counted)
              entry.active--;
            entry.touched = Date.now();
          }
        }
        return;
      }
      const methods = { '/control/status': 'GET', '/control/profiles': 'POST', '/control/test': 'POST', '/control/events': 'GET', '/control/logs': 'GET', '/control/auth': 'POST', '/control/shutdown': 'POST' };
      if (!methods[url.pathname]) {
        json(response, 404, { error: '接口不存在' });
        return;
      }
      if (request.method !== methods[url.pathname]) {
        json(response, 405, { error: '请求方法无效' });
        return;
      }
      const body = request.method === 'POST' ? await readJson(request) : undefined;
      if (request.method === 'POST' && (!body || typeof body !== 'object' || Array.isArray(body)))
        throw new EngineError('INVALID_BODY', '请求无效');
      switch (url.pathname) {
        case '/control/status':
          json(response, 200, { running: true, version: '0.1.0', pid: process.pid, endpoint: engine.url + '/mcp' });
          break;
        case '/control/profiles': {
          // Serialize validation and commit to retain request order and atomicity.
          const applied = updates.then(async () => {
            const profiles = await validateProfiles(body.profiles);
            if (stopping)
              throw new EngineError('ENGINE_STOPPED', '服务已停止', 503);
            const count = pool.update(profiles);
            diagnostics.record('profiles_synced');
            return count;
          });
          updates = applied.catch(() => {
          });
          json(response, 200, { count: await applied });
          break;
        }
        case '/control/test':
          if (typeof body.profileId !== 'string' || !UUID.test(body.profileId))
            throw new EngineError('INVALID_ID', '连接 ID 无效');
          json(response, 200, await pool.test(body.profileId.toLowerCase()));
          break;
        case '/control/logs': {
          const since = url.searchParams.get('since') ?? '0';
          if (!/^(0|[1-9][0-9]*)$/.test(since) || !Number.isSafeInteger(Number(since)))
            throw new EngineError('INVALID_CURSOR', '日志游标无效');
          json(response, 200, diagnostics.since(Number(since)));
          break;
        }
        case '/control/events': {
          const since = url.searchParams.get('since') ?? '0';
          if (!/^(0|[1-9][0-9]*)$/.test(since) || !Number.isSafeInteger(Number(since)))
            throw new EngineError('INVALID_CURSOR', '事件游标无效');
          json(response, 200, auth.events(Number(since)));
          break;
        }
        case '/control/auth':
          if (typeof body.requestId !== 'string' || !UUID.test(body.requestId) || (body.cancel !== undefined && typeof body.cancel !== 'boolean') || (body.cancel === true && body.code !== undefined) || (body.cancel !== true && (typeof body.code !== 'string' || !body.code.length || body.code.length > 1024 || /[\x00-\x1f\x7f]/.test(body.code))))
            throw new EngineError('INVALID_AUTH_RESPONSE', '认证回答无效');
          {
            const accepted = auth.respond(body);
            json(response, 200, { accepted });
          }
          break;
        case '/control/shutdown':
          json(response, 200, { accepted: true });
          setImmediate(() => void close());
          break;
      }
    }
    catch (error) {
      json(response, error instanceof EngineError ? error.status : 500, { error: error instanceof EngineError ? error.message : '操作失败' });
    }
  });
  server.requestTimeout = 15000;
  server.headersTimeout = 10000;
  server.keepAliveTimeout = 5000;
  const sockets = new Set();
  server.on('connection', socket => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
  });
  async function close() {
    if (closePromise)
      return closePromise;
    stopping = true;
    clearInterval(timer);
    auth.close();
    pool.close();
    closePromise = (async () => {
      const listenerClosed = new Promise(resolve => server.close(resolve));
      const deadline = setTimeout(() => {
        for (const socket of sockets)
          socket.destroy();
      }, 1000);
      deadline.unref();
      await Promise.allSettled([...sessions].map(([id, entry]) => closeSession(id, entry)));
      server.closeIdleConnections();
      await listenerClosed;
      clearTimeout(deadline);
      resolveClosed();
    })();
    return closePromise;
  }
  const engine = { server, pool, auth, diagnostics, sessions, closed, close };
  try {
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(port, remoteHost ? (isIP(remoteHost) === 6 ? '::' : '0.0.0.0') : '127.0.0.1', resolve);
    });
  }
  catch (error) {
    clearInterval(timer);
    auth.close();
    pool.close();
    throw new EngineError('LISTEN_FAILED', '服务端口不可用', 503);
  }
  engine.address = server.address();
  if (remoteHost) remoteAuthority = `${remoteHost.includes(':') ? `[${remoteHost}]` : remoteHost}:${engine.address.port}`;
  engine.url = 'http://127.0.0.1:' + engine.address.port;
  diagnostics.record('engine_ready');
  return engine;
}
