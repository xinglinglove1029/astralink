import test from 'node:test';
import assert from 'node:assert/strict';
import { createSessionClient } from '../src/session-client.mjs';
import { createEngine } from '../src/engine.mjs';
import { eventually, fakeSsh, profile, request, tokens } from './helpers.mjs';

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const message = init => init?.method === 'POST' ? JSON.parse(init.body) : undefined;
async function setup(t, options = {}) {
  const engine = await createEngine({ ...tokens, port: 0 });
  t.after(() => engine.close());
  const client = createSessionClient({ url: engine.url + '/mcp', token: tokens.mcpToken, heartbeatIntervalMs: 0, ...options });
  t.after(() => client.close());
  await client.connect();
  return { client, engine };
}

test('idle heartbeat refreshes the same session and stops when the bridge closes', async t => {
  let pings = 0;
  const { client, engine } = await setup(t, {
    heartbeatIntervalMs: 25,
    fetchImpl: (input, init) => {
      if (message(init)?.method === 'ping') pings++;
      return fetch(input, init);
    },
  });
  const [id, entry] = [...engine.sessions][0];
  entry.touched = Date.now() - 1800001;
  await eventually(() => pings >= 3 && Date.now() - entry.touched < 1000);
  assert.equal(engine.sessions.size, 1);
  assert.ok(engine.sessions.has(id));
  await client.close();
  const stopped = pings;
  await pause(80);
  assert.equal(pings, stopped);
  await assert.rejects(client.listTools());
});

test('authentication, server failures and unrelated 404s are never replayed', async t => {
  let blocked = 0, initializations = 0, calls = 0;
  const { client } = await setup(t, {
    fetchImpl: (input, init) => {
      const method = message(init)?.method;
      if (method === 'initialize') initializations++;
      if (method === 'tools/call') {
        calls++;
        if (blocked) return Promise.resolve(new Response(JSON.stringify({ error: 'unrelated failure' }), { status: blocked }));
      }
      return fetch(input, init);
    },
  });
  for (const status of [401, 403, 404, 500]) {
    blocked = status;
    await assert.rejects(client.callTool({ name: 'list-servers', arguments: {} }));
  }
  assert.equal(calls, 4);
  assert.equal(initializations, 1);
  blocked = 0;
  assert.ok(!(await client.callTool({ name: 'list-servers', arguments: {} })).isError);
});

test('a lost response after real SSH execution is not retried', async t => {
  const ssh = await fakeSsh({ password: 'synthetic-password' });
  t.after(() => ssh.close());
  let sent = 0;
  const { client, engine } = await setup(t, {
    fetchImpl: async (input, init) => {
      const response = await fetch(input, init);
      if (message(init)?.method === 'tools/call') {
        sent++;
        await response.arrayBuffer();
        throw new TypeError('simulated response lost after execution');
      }
      return response;
    },
  });
  await request(engine, '/control/profiles', { profiles: [profile({ port: ssh.port, password: 'synthetic-password' })] });
  await assert.rejects(client.callTool({ name: 'execute-command', arguments: { connectionName: 'test', cmdString: 'hostname && id' } }));
  assert.equal(sent, 1);
  assert.deepEqual(ssh.seen.commands, ['hostname && id']);
});

test('session recovery retries only once, and a later call can reconnect', async t => {
  let expired = true, calls = 0, initializations = 0;
  const { client, engine } = await setup(t, {
    fetchImpl: async (input, init) => {
      const method = message(init)?.method;
      if (method === 'initialize') initializations++;
      if (method === 'tools/list') {
        calls++;
        if (expired) {
          const id = new Headers(init.headers).get('mcp-session-id');
          const entry = engine.sessions.get(id);
          if (entry) await entry.mcp.close();
          engine.sessions.delete(id);
          return new Response(JSON.stringify({ error: '会话不存在', code: 'SESSION_NOT_FOUND' }), { status: 404 });
        }
      }
      return fetch(input, init);
    },
  });
  await assert.rejects(client.listTools());
  assert.equal(initializations, 2);
  assert.equal(calls, 2);
  expired = false;
  assert.equal((await client.listTools()).tools.length, 10);
  assert.equal(initializations, 3);
});

test('cancelled calls are not replayed while a replacement session initializes', async t => {
  let delaying = false, release, entered;
  const gate = new Promise(resolve => { release = resolve; });
  const initializing = new Promise(resolve => { entered = resolve; });
  let initializations = 0, calls = 0;
  const { client, engine } = await setup(t, {
    fetchImpl: async (input, init) => {
      const method = message(init)?.method;
      if (method === 'initialize') {
        initializations++;
        if (delaying) { entered(); await gate; }
      }
      if (method === 'tools/call') calls++;
      return fetch(input, init);
    },
  });
  t.after(release);
  const [id, entry] = [...engine.sessions][0];
  await entry.mcp.close();
  engine.sessions.delete(id);
  delaying = true;
  const abort = new AbortController();
  const cancelled = client.callTool({ name: 'list-servers', arguments: {} }, { signal: abort.signal });
  const rejected = assert.rejects(cancelled);
  await initializing;
  abort.abort();
  await rejected; // Cancellation must settle without waiting for the shared initialization.
  const concurrent = client.listTools();
  release();
  assert.equal((await concurrent).tools.length, 10);
  assert.equal(initializations, 2);
  assert.equal(calls, 1); // The sole tool request was rejected before dispatch on the old session.
});

test('reconnecting one call does not abort another in-flight call on the old session', async t => {
  let release, entered;
  const gate = new Promise(resolve => { release = resolve; });
  const executing = new Promise(resolve => { entered = resolve; });
  const { client, engine } = await setup(t, {
    fetchImpl: async (input, init) => {
      const response = await fetch(input, init);
      if (message(init)?.method === 'tools/call') { entered(); await gate; }
      return response;
    },
  });
  t.after(release);
  const pending = client.callTool({ name: 'list-servers', arguments: {} });
  await executing;
  const [id, entry] = [...engine.sessions][0];
  // Keep the old response alive while simulating a missing session for the next request.
  engine.sessions.delete(id);
  t.after(() => entry.mcp.close());
  assert.equal((await client.listTools()).tools.length, 10);
  release();
  assert.ok(!(await pending).isError);
  assert.equal(engine.sessions.size, 1);
});

test('a tool timeout does not replay a command that the SSH server already executed', async t => {
  const ssh = await fakeSsh({ password: 'synthetic-password' });
  t.after(() => ssh.close());
  let release, entered, sent = 0;
  const gate = new Promise(resolve => { release = resolve; });
  const executing = new Promise(resolve => { entered = resolve; });
  t.after(release);
  const { client, engine } = await setup(t, {
    fetchImpl: async (input, init) => {
      const response = await fetch(input, init);
      if (message(init)?.method === 'tools/call') { sent++; entered(); await gate; }
      return response;
    },
  });
  await request(engine, '/control/profiles', { profiles: [profile({ port: ssh.port, password: 'synthetic-password' })] });
  const called = client.callTool({ name: 'execute-command', arguments: { connectionName: 'test', cmdString: 'hostname && id' } }, { timeout: 500 });
  const rejected = assert.rejects(called, error => error.code === -32001);
  await executing;
  await rejected;
  assert.equal(sent, 1);
  assert.deepEqual(ssh.seen.commands, ['hostname && id']);
  release();
});
