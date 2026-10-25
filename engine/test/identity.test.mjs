import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { createEngine } from '../src/engine.mjs';
import { recordToolCall } from '../src/audit.mjs';
import { tokens, request } from './helpers.mjs';

test('server MCP tokens identify callers, isolate sessions and revoke immediately', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'astralink-mcp-identities-'));
  const identityFile = join(dir, 'identities.json'), auditFile = join(dir, 'audit.jsonl');
  const tokenA = 'personal-token-user-a', tokenB = 'personal-token-user-b';
  const entry = (id, username, token) => ({ id, username, role: 'operator', hash: createHash('sha256').update(token).digest('hex') });
  writeFileSync(identityFile, JSON.stringify([entry('user-a', 'alice', tokenA), entry('user-b', 'bob', tokenB)]));
  const engine = await createEngine({ ...tokens, port: 0, identitiesFile: identityFile, auditFile: auditFile });
  t.after(async () => { await engine.close(); rmSync(dir, { recursive: true, force: true }); });
  const legacy = await request(engine, '/mcp', { jsonrpc: '2.0', id: 1, method: 'initialize', params: {} },
    { authorization: 'Bearer ' + tokens.mcpToken });
  assert.equal(legacy.status, 401);
  const client = new Client({ name: 'identity-test', version: '1.0' });
  const transport = new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokenA } } });
  await client.connect(transport);
  t.after(() => client.close());
  await client.callTool({ name: 'list-servers', arguments: {} });
  const records = readFileSync(auditFile, 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.ok(records.some(record => record.actorId === 'user-a' && record.actor === 'alice' && record.action === 'list-servers' && record.outcome === 'success'));
  assert.ok(!readFileSync(auditFile, 'utf8').includes(tokenA));
  const stolen = await request(engine, '/mcp', { jsonrpc: '2.0', id: 5, method: 'tools/list', params: {} },
    { authorization: 'Bearer ' + tokenB, 'mcp-session-id': transport.sessionId, accept: 'application/json, text/event-stream' });
  assert.equal(stolen.status, 403);
  writeFileSync(identityFile, JSON.stringify([entry('user-b', 'bob', tokenB)]));
  const denied = await request(engine, '/mcp', { jsonrpc: '2.0', id: 6, method: 'tools/list', params: {} },
    { authorization: 'Bearer ' + tokenA, 'mcp-session-id': transport.sessionId, accept: 'application/json, text/event-stream' });
  assert.equal(denied.status, 401);
  writeFileSync(identityFile, '{not-json');
  const corrupt = await request(engine, '/mcp', { jsonrpc: '2.0', id: 7, method: 'tools/list', params: {} },
    { authorization: 'Bearer ' + tokenB, accept: 'application/json, text/event-stream' });
  assert.equal(corrupt.status, 401);
});

test('MCP audit rotates without storing tool input or response', t => {
  const dir = mkdtempSync(join(tmpdir(), 'astralink-mcp-audit-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const file = join(dir, 'audit.jsonl');
  writeFileSync(file, Buffer.alloc(8 * 1024 * 1024 + 1, 32));
  recordToolCall(file, { actorId: 'u1', actor: 'alice', action: 'ssh-exec', outcome: 'failure', profileId: 'p1', durationMs: 42, params: 'secret-input' });
  assert.equal(readFileSync(`${file}.1`).length, 8 * 1024 * 1024 + 1);
  const entry = JSON.parse(readFileSync(file, 'utf8'));
  assert.deepEqual(Object.keys(entry), ['timestamp', 'actorId', 'actor', 'channel', 'action', 'outcome', 'profileId', 'durationMs']);
  assert.equal(entry.actor, 'alice');
  assert.ok(!readFileSync(file, 'utf8').includes('secret-input'));
});
