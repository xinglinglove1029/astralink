import test from 'node:test';
import http from 'node:http';
import { networkInterfaces } from 'node:os';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createEngine } from '../src/engine.mjs';
import { tokens, request } from './helpers.mjs';

test('remote MCP opt-in validates host and personal auth while keeping control private', async t => {
  await assert.rejects(createEngine({ ...tokens, port: 0, remoteHost: '172.17.112.203' }));
  const dir = mkdtempSync(join(tmpdir(), 'astralink-remote-'));
  const identitiesFile = join(dir, 'identities.json'), auditFile = join(dir, 'audit.jsonl');
  const token = 'remote-synthetic-personal-token';
  writeFileSync(identitiesFile, JSON.stringify([{ id: 'user-a', username: 'alice', role: 'operator', hash: createHash('sha256').update(token).digest('hex') }]));
  const engine = await createEngine({ ...tokens, port: 0, remoteHost: '172.17.112.203', identitiesFile, auditFile });
  t.after(async () => { await engine.close(); rmSync(dir, { recursive: true, force: true }); });
  assert.equal(engine.address.address, '0.0.0.0');
  const host = '172.17.112.203:' + engine.address.port;
  assert.equal((await request(engine, '/control/status', undefined, { host })).status, 403);
  assert.equal((await request(engine, '/mcp', {}, { host, 'content-type': 'application/json' })).status, 401);
  assert.equal((await request(engine, '/mcp', {}, { host: 'evil.example', authorization: 'Bearer ' + token })).status, 403);
  assert.equal((await request(engine, '/mcp', {}, { host, origin: 'http://evil.example', authorization: 'Bearer ' + token })).status, 403);
  const initialized = await request(engine, '/mcp', { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'remote-smoke', version: '1' } } }, { host, authorization: 'Bearer ' + token, accept: 'application/json, text/event-stream', 'content-type': 'application/json' });
  assert.equal(initialized.status, 200);
  const session = initialized.headers['mcp-session-id'];
  const called = await request(engine, '/mcp', { jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'list-servers', arguments: {} } }, { host, 'mcp-session-id': session, authorization: 'Bearer ' + token, accept: 'application/json, text/event-stream', 'content-type': 'application/json' });
  assert.equal(called.status, 200);
  assert.ok(readFileSync(auditFile, 'utf8').includes('alice'));
  assert.ok(!readFileSync(auditFile, 'utf8').includes(token));
  const lan = Object.values(networkInterfaces()).flat().find(info => info?.family === 'IPv4' && !info.internal)?.address;
  assert.ok(lan, 'A non-loopback IPv4 interface is required for the remote boundary check');
  const probe = (path, authority, bearer) => new Promise((resolve, reject) => {
    const req = http.get({ host: lan, port: engine.address.port, path, headers: { Host: authority, Authorization: 'Bearer ' + bearer } }, res => {
      res.resume(); res.on('end', () => resolve(res.statusCode));
    });
    req.on('error', reject);
  });
  // Docker/remote peers cannot access control even with the control credential or a loopback Host.
  assert.equal(await probe('/control/status', host, tokens.controlToken), 403);
  assert.equal(await probe('/control/status', '127.0.0.1:' + engine.address.port, tokens.controlToken), 403);
  assert.equal(await probe('/mcp', host, 'invalid-token'), 401);

});
