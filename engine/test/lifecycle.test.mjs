import test from 'node:test';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import http from 'node:http';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { feature, profile, tokens, request, fakeSsh, eventually } from './helpers.mjs';
async function start(t, options = {}) {
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0, ...options });
  t.after(() => engine.close());
  return engine;
}
async function clientFor(t, engine) {
  const client = new Client({ name: 'lifecycle', version: '1.0' });
  const transport = new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokens.mcpToken } } });
  await client.connect(transport);
  t.after(() => client.close());
  return { client, transport };
}
test('installed SDK resolution uses patched version for engine and upstream', () => {
  const require = createRequire(import.meta.url);
  const upstreamRequire = createRequire(require.resolve('@fangjunjie/ssh-mcp-server'));
  const sdkVersion = resolver => JSON.parse(readFileSync(path.resolve(path.dirname(resolver.resolve('@modelcontextprotocol/sdk/server/mcp.js')), '../../../package.json'), 'utf8')).version;
  assert.equal(sdkVersion(require), '1.32.1');
  assert.equal(sdkVersion(upstreamRequire), '1.32.1');
});
test('session capacity and expiry enforce bounded lifecycle', async (t) => {
  const engine = await start(t, { maxSessions: 1, sessionTtlMs: 70 });
  const { transport } = await clientFor(t, engine);
  const denied = await request(engine, '/mcp', { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'extra', version: '1' } } }, { authorization: 'Bearer ' + tokens.mcpToken, accept: 'application/json, text/event-stream' });
  assert.equal(denied.status, 429);
  await eventually(() => engine.sessions.size === 0);
  assert.equal((await request(engine, '/mcp', undefined, { authorization: 'Bearer ' + tokens.mcpToken, 'mcp-session-id': transport.sessionId })).status, 404);
});
test('engine instances retain distinct upstream manager contexts and exact tool schemas', async (t) => {
  const first = await start(t), second = await start(t);
  await request(first, '/control/profiles', { profiles: [profile({ name: 'one' })] });
  await request(second, '/control/profiles', { profiles: [profile({ name: 'two' })] });
  const a = await clientFor(t, first), b = await clientFor(t, second);
  const results = await Promise.all([a.client.callTool({ name: 'list-servers', arguments: {} }), b.client.callTool({ name: 'list-servers', arguments: {} })]);
  assert.match(results[0].content[0].text, /one/);
  assert.ok(!results[0].content[0].text.includes('two'));
  assert.match(results[1].content[0].text, /two/);
  const { registerAllTools } = await import('@fangjunjie/ssh-mcp-server/build/tools/index.js');
  const { McpServer } = await import('@modelcontextprotocol/sdk/server/mcp.js');
  const { InMemoryTransport } = await import('@modelcontextprotocol/sdk/inMemory.js');
  const reference = new McpServer({ name: 'upstream', version: '1' });
  registerAllTools(reference);
  const referenceClient = new Client({ name: 'reference', version: '1' });
  const [ct, st] = InMemoryTransport.createLinkedPair();
  await reference.connect(st);
  await referenceClient.connect(ct);
  t.after(() => referenceClient.close());
  t.after(() => reference.close());
  assert.deepEqual(JSON.parse(JSON.stringify((await a.client.listTools()).tools)), JSON.parse(JSON.stringify((await referenceClient.listTools()).tools)));
});
test('OTP queues isolate owners during connection retirement and prohibit impossible history limits', async () => {
  const { AuthQueue } = await feature('auth');
  assert.throws(() => new AuthQueue({ maxPending: 3, maxHistory: 2 }));
  const auth = new AuthQueue();
  const p = profile();
  const old = auth.challenge(p, '', [{ prompt: 'OTP', echo: false }], 'old');
  const oldRejected = assert.rejects(old, { code: 'AUTH_CANCELLED' });
  const fresh = auth.challenge(p, '', [{ prompt: 'OTP', echo: false }], 'fresh');
  auth.cancelOwner('old');
  await oldRejected;
  const event = auth.events(0).events[0];
  assert.equal(auth.respond({ requestId: event.requestId, code: 'fresh-code' }), true);
  assert.deepEqual(await fresh, ['fresh-code']);
  auth.close();
});
test('shutdown interrupts real pending authentication and no credentials or tokens reach logs', async (t) => {
  const ssh = await fakeSsh();
  t.after(() => ssh.close());
  const engine = await start(t);
  const p = profile({ port: ssh.port, password: 'credential-sentinel', passphrase: 'passphrase-sentinel' });
  await request(engine, '/control/profiles', { profiles: [p] });
  const chunks = [];
  const original = process.stderr.write;
  process.stderr.write = function (chunk, ...args) {
    chunks.push(String(chunk));
    return original.call(this, chunk, ...args);
  };
  t.after(() => {
    process.stderr.write = original;
  });
  const pending = request(engine, '/control/test', { profileId: p.id });
  await eventually(() => engine.auth.pending.size === 1);
  await request(engine, '/control/shutdown', {});
  await engine.closed;
  const result = await pending;
  assert.equal(result.body.status, 'auth_required');
  const logs = chunks.join('');
  for (const secret of [p.password, p.passphrase, tokens.mcpToken, tokens.controlToken])
    assert.ok(!logs.includes(secret));
  assert.equal(engine.auth.pending.size, 0);
});
test('actual command failure reports command stage, not connection stage', async (t) => {
  const ssh = await fakeSsh({ password: 'probe-pass', probeExit: 9 });
  t.after(() => ssh.close());
  const engine = await start(t);
  const p = profile({ port: ssh.port, password: 'probe-pass', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [p] });
  const result = await request(engine, '/control/test', { profileId: p.id });
  assert.equal(result.body.status, 'failed');
  assert.equal(result.body.stage, 'command');
});
test('standalone entrypoint resolves resources independently of cwd and keeps startup diagnostics generic', async (t) => {
  const reserve = http.createServer();
  await new Promise(resolve => reserve.listen(0, '127.0.0.1', resolve));
  const port = reserve.address().port;
  await new Promise(resolve => reserve.close(resolve));
  const main = fileURLToPath(new URL('../src/main.mjs', import.meta.url));
  const child = spawn(process.execPath, [main], { cwd: tmpdir(), env: { ...process.env, ASTRALINK_PORT: String(port), ASTRALINK_CONTROL_TOKEN: 'child-control-' + randomUUID(), ASTRALINK_MCP_TOKEN: 'child-mcp-' + randomUUID() }, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', data => stdout += data);
  child.stderr.on('data', data => stderr += data);
  t.after(() => child.kill());
  const exited = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
  const control = child.spawnargs;
  assert.ok(control.includes(main));
  await eventually(async () => {
    try {
      const res = await fetch('http://127.0.0.1:' + port + '/control/status');
      return res.status === 401;
    }
    catch {
      return false;
    }
  });
  child.kill();
  await exited;
  assert.equal(stdout, '');
  assert.equal(stderr, '');
  const invalid = spawn(process.execPath, [main], { env: { ...process.env, ASTRALINK_MCP_TOKEN: 'startup-secret', ASTRALINK_CONTROL_TOKEN: 'startup-secret' }, stdio: ['ignore', 'pipe', 'pipe'] });
  let errors = '';
  invalid.stderr.on('data', data => errors += data);
  const code = await new Promise(resolve => invalid.once('exit', resolve));
  assert.equal(code, 1);
  assert.ok(!errors.includes('startup-secret'));
  assert.match(errors, /启动失败/);
});
test('MCP DELETE closes SDK protocol state as well as removing the HTTP session', async (t) => {
  const engine = await start(t);
  const { transport } = await clientFor(t, engine);
  const entry = engine.sessions.get(transport.sessionId);
  assert.ok(entry.mcp.server.transport);
  await transport.terminateSession();
  assert.equal(engine.sessions.size, 0);
  assert.equal(entry.mcp.server.transport, undefined);
});
