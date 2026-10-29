import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

class SessionExpiredError extends Error {
  constructor() { super('MCP 会话已失效'); }
}

/** One shared HTTP session for a stdio bridge, with bounded, pre-dispatch recovery. */
export function createSessionClient({ url, token, heartbeatIntervalMs = 300000, fetchImpl = fetch }) {
  let current, connecting, closed = false, heartbeatBusy = false;
  const entries = new Set(), lifetime = new AbortController();
  function closeEntry(entry) {
    entry.closing ??= entry.client.close().catch(() => {}).finally(() => entries.delete(entry));
    return entry.closing;
  }
  async function open() {
    const client = new Client({ name: 'astralink-local-bridge', version: '0.1.1' });
    const entry = { client, active: 0, retired: false };
    entries.add(entry);
    const transport = new StreamableHTTPClientTransport(new URL(url), {
      requestInit: { headers: { Authorization: `Bearer ${token}` } },
      fetch: async (input, init) => {
        const response = await fetchImpl(input, init);
        if (init?.method === 'POST' && new Headers(init.headers).has('mcp-session-id') && response.status === 404) {
          const body = await response.clone().json().catch(() => null);
          // This engine response is emitted before tool dispatch. Generic 404s are not safe to replay.
          if (body?.code === 'SESSION_NOT_FOUND' || body?.error === '会话不存在') {
            await response.body?.cancel();
            throw new SessionExpiredError();
          }
        }
        return response;
      },
    });
    try {
      await client.connect(transport, { signal: lifetime.signal, timeout: 10000 });
      lifetime.signal.throwIfAborted();
      current = entry;
      return entry;
    } catch (error) {
      await closeEntry(entry);
      throw error;
    }
  }
  function acquire() {
    if (closed) return Promise.reject(new Error('MCP 桥接已关闭'));
    if (current) return Promise.resolve(current);
    connecting ??= open().finally(() => { connecting = undefined; });
    return connecting;
  }
  function acquireFor(signal) {
    signal.throwIfAborted();
    const session = acquire();
    return new Promise((resolve, reject) => {
      const aborted = () => reject(signal.reason);
      signal.addEventListener('abort', aborted, { once: true });
      session.then(resolve, reject).finally(() => signal.removeEventListener('abort', aborted));
      if (signal.aborted) aborted();
    });
  }
  async function execute(operation, options = {}) {
    const signal = options.signal ? AbortSignal.any([options.signal, lifetime.signal]) : lifetime.signal;
    for (let attempt = 0; attempt < 2; attempt++) {
      signal.throwIfAborted();
      const entry = await acquireFor(signal);
      signal.throwIfAborted();
      entry.active++;
      try {
        return await operation(entry.client, { ...options, signal });
      } catch (error) {
        if (!(error instanceof SessionExpiredError)) throw error;
        if (current === entry) current = undefined;
        entry.retired = true;
        if (attempt || signal.aborted) throw error;
      } finally {
        entry.active--;
        // Other calls on this generation may still finish; never cancel them to reconnect.
        if (entry.retired && !entry.active) void closeEntry(entry);
      }
    }
  }
  const timer = heartbeatIntervalMs > 0 ? setInterval(() => {
    if (closed || !current || current.active || connecting || heartbeatBusy) return;
    heartbeatBusy = true;
    void execute((client, options) => client.ping(options), { timeout: 10000 })
      .catch(() => {}).finally(() => { heartbeatBusy = false; });
  }, heartbeatIntervalMs) : undefined;
  timer?.unref();
  return {
    connect: async () => { await acquire(); },
    listTools: (params, options) => execute((client, requestOptions) => client.listTools(params, requestOptions), options),
    callTool: (params, options) => execute((client, requestOptions) => client.callTool(params, undefined, requestOptions), options),
    async close() {
      closed = true;
      clearInterval(timer);
      lifetime.abort();
      await Promise.allSettled([...entries].map(closeEntry));
      await connecting?.catch(() => {});
    },
  };
}
