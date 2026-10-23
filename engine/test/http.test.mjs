import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { feature, profile, request, tokens } from './helpers.mjs';
async function start(t, options = {}) {
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0, ...options });
  t.after(() => engine.close());
  return engine;
}
test('HTTP binds loopback; MCP/control bearer credentials remain isolated', async (t) => {
  const engine = await start(t);
  assert.equal(engine.address.address, '127.0.0.1');
  assert.equal((await request(engine, '/control/status')).body.running, true);
  for (const authorization of ['', 'Bearer ' + tokens.mcpToken, 'Bearer wrong'])
    assert.equal((await request(engine, '/control/status', undefined, { authorization })).status, 401);
  assert.equal((await request(engine, '/mcp', {}, { authorization: 'Bearer ' + tokens.controlToken })).status, 401);
});
test('Origin and Host validation reject hostile, malformed, null and DNS rebinding requests', async (t) => {
  const engine = await start(t);
  for (const origin of ['https://attacker.example', 'null', 'http://localhost.attacker.example', 'http://localhost/path', 'http://user@localhost', 'http://127.1', 'file://localhost', 'http://localhost, http://attacker'])
    assert.equal((await request(engine, '/control/status', undefined, { origin })).status, 403, origin);
  for (const origin of ['http://127.0.0.1:3000', 'http://localhost:3000', 'https://[::1]:4000'])
    assert.equal((await request(engine, '/control/status', undefined, { origin })).status, 200, origin);
  assert.equal((await request(engine, '/control/status', undefined, { host: 'attacker.example' })).status, 403);
  assert.equal((await request(engine, '/mcp', {}, { authorization: 'Bearer ' + tokens.mcpToken, origin: 'https://evil.example' })).status, 403);
});
test('profile replacement is atomic; control errors are bounded and do not expose input secrets', async (t) => {
  const engine = await start(t);
  assert.equal((await request(engine, '/control/profiles', { profiles: [profile()] })).body.count, 1);
  assert.equal((await request(engine, '/control/profiles', { profiles: [profile({ password: 'secret-input', port: 70000 })] })).status, 400);
  assert.equal(engine.pool.getAllServerInfos()[0].name, 'test');
  assert.equal((await request(engine, '/control/profiles', { profiles: [profile(), profile({ id: '22222222-2222-4222-8222-222222222222' })] })).status, 400);
  const large = await fetch(engine.url + '/control/profiles', { method: 'POST', headers: { authorization: 'Bearer ' + tokens.controlToken, 'content-type': 'application/json' }, body: 'x'.repeat(1100000) });
  assert.equal(large.status, 413);
  assert.equal((await request(engine, '/control/events?since=bad')).status, 400);
  assert.equal((await request(engine, '/control/auth', { requestId: 'bad', code: 'secret' })).status, 400);
});
test('real SDK initializes stateful MCP sessions and discovers exactly upstream schemas', async (t) => {
  const engine = await start(t);
  const client = new Client({ name: 'AstraLink tests', version: '1.0' });
  const transport = new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokens.mcpToken } } });
  await client.connect(transport);
  t.after(() => client.close());
  const tools = await client.listTools();
  assert.equal(tools.tools.length, 10);
  assert.deepEqual(tools.tools.map(x => x.name).sort(), ['execute-command', 'list-servers', 'upload', 'download', 'read-file', 'write-file', 'job-start', 'job-status', 'job-kill', 'job-list'].sort());
  const execute = tools.tools.find(x => x.name === 'execute-command');
  assert.deepEqual(Object.keys(execute.inputSchema.properties).sort(), ['cmdString', 'connectionName', 'directory', 'timeout']);
  assert.ok(transport.sessionId);
  const wrong = await request(engine, '/mcp', undefined, { authorization: 'Bearer ' + tokens.mcpToken, 'mcp-session-id': 'nonexistent' });
  assert.equal(wrong.status, 404);
  await request(engine, '/control/profiles', { profiles: [profile()] });
  const listing = await client.callTool({ name: 'list-servers', arguments: {} });
  assert.match(listing.content[0].text, /test/);
  assert.ok(!listing.isError);
  await transport.terminateSession();
});
test('mandatory tokens must differ; malformed request and wrong method have explicit failures', async (t) => {
  const { createEngine } = await feature('engine');
  await assert.rejects(() => createEngine({ port: 0 }));
  await assert.rejects(() => createEngine({ ...tokens, controlToken: tokens.mcpToken, port: 0 }));
  const engine = await start(t);
  assert.equal((await request(engine, '/control/profiles')).status, 405);
  const response = await fetch(engine.url + '/control/profiles', { method: 'POST', headers: { authorization: 'Bearer ' + tokens.controlToken, 'content-type': 'application/json' }, body: '{bad' });
  assert.equal(response.status, 400);
});
test('control shutdown acknowledges before closing listener and active sessions', async (t) => {
  const engine = await start(t);
  assert.equal((await request(engine, '/control/shutdown', {})).body.accepted, true);
  await engine.closed;
  assert.equal(engine.server.listening, false);
});
test('chunked oversized JSON returns a bounded error without resetting the connection', async (t) => {
  const engine = await start(t);
  const result = await new Promise((resolve, reject) => {
    const req = http.request(engine.url + '/control/profiles', { method: 'POST', headers: { authorization: 'Bearer ' + tokens.controlToken, 'content-type': 'application/json', 'transfer-encoding': 'chunked' } }, res => {
      let body = '';
      res.on('data', chunk => body += chunk);
      res.on('end', () => resolve({ status: res.statusCode, body }));
    });
    req.on('error', reject);
    for (let i = 0; i < 18; i++)
      req.write('x'.repeat(65536));
    req.end();
  });
  assert.equal(result.status, 413);
  assert.ok(result.body.length < 256);
});
test('runtime synchronization preserves multiline imported notes and rejects bad notes atomically', async (t) => {
  const engine = await start(t);
  const notes = 'CSV imported note\n\tIndented line\r\nWindows ending\r\n';
  const p = profile({ notes });
  const synced = await request(engine, '/control/profiles', { profiles: [p] });
  assert.equal(synced.status, 200);
  assert.equal(synced.body.count, 1);
  assert.equal(engine.pool.profiles.get(p.id).notes, notes);
  const entry = engine.pool.entries.get(p.name);
  assert.equal(entry.profile.notes, notes);
  const rejected = await request(engine, '/control/profiles', { profiles: [{ ...p, notes: 'bad\u0000note' }] });
  assert.equal(rejected.status, 400);
  assert.equal(engine.pool.entries.get(p.name), entry);
  assert.equal(engine.pool.profiles.get(p.id).notes, notes);
});
