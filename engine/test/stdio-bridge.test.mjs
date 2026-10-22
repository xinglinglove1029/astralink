import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { feature, tokens, request, profile } from './helpers.mjs';

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
