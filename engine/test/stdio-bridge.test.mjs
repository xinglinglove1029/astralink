import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { feature, tokens, request, profile, eventually } from './helpers.mjs';

test('stdio bridge exposes the local HTTP tools and forwards calls', async (t) => {
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0 });
  t.after(() => engine.close());
  await request(engine, '/control/profiles', { profiles: [profile()] });
  const client = new Client({ name: 'desktop-bridge-test', version: '1' });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [fileURLToPath(new URL('../src/stdio-bridge.mjs', import.meta.url))],
    env: { ...process.env, ASTRALINK_MCP_PORT: String(engine.address.port), ASTRALINK_MCP_TOKEN: tokens.mcpToken },
  });
  await client.connect(transport);
  t.after(() => client.close());
  assert.equal((await client.listTools()).tools.length, 10);
  const result = await client.callTool({ name: 'list-servers', arguments: {} });
  assert.match(result.content[0].text, /test/);
  assert.ok(!result.isError);
});

test('stdio bridge recovers one shared HTTP session after idle expiry and an engine restart', async (t) => {
  const { createEngine } = await feature('engine');
  let engine = await createEngine({ ...tokens, port: 0, maxSessions: 1, sessionTtlMs: 1000 });
  t.after(() => engine.close());
  const port = engine.address.port;
  const client = new Client({ name: 'bridge-recovery-test', version: '1' });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [fileURLToPath(new URL('../src/stdio-bridge.mjs', import.meta.url))],
    env: { ...process.env, ASTRALINK_MCP_PORT: String(port), ASTRALINK_MCP_TOKEN: tokens.mcpToken },
  });
  await client.connect(transport);
  t.after(() => client.close());
  await client.listTools();
  const oldId = [...engine.sessions.keys()][0];
  engine.sessions.get(oldId).touched = Date.now() - 1001;
  await eventually(() => engine.sessions.size === 0);
  const results = await Promise.all(Array.from({ length: 8 }, () => client.listTools()));
  assert.ok(results.every(result => result.tools.length === 10));
  assert.equal(engine.sessions.size, 1);
  assert.ok(!engine.sessions.has(oldId));
  await engine.close();
  engine = await createEngine({ ...tokens, port, maxSessions: 1 });
  await request(engine, '/control/profiles', { profiles: [profile({ name: 'after-restart' })] });
  const called = await client.callTool({ name: 'list-servers', arguments: {} });
  assert.ok(!called.isError);
  assert.match(called.content[0].text, /after-restart/);
  assert.equal(engine.sessions.size, 1);
});
