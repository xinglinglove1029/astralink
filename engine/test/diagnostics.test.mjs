import test from 'node:test';
import assert from 'node:assert/strict';
import { feature, profile, tokens, request } from './helpers.mjs';

test('diagnostics accept fixed events and UUID context only, bound history, and detect missing history', async () => {
  const { Diagnostics } = await feature('diagnostics');
  const logs = new Diagnostics(2);
  logs.record('ssh_connecting', profile().id, '11111111-1111-4111-8111-111111111111');
  logs.record('auth_otp_echo', profile().id);
  logs.record('otp_auto', profile().id);
  logs.record('password=private-secret', profile().id);
  logs.record('otp_auto', 'private-secret');
  const batch = logs.since(0);
  assert.equal(batch.entries.length, 2);
  assert.equal(batch.dropped, true);
  assert.equal(batch.cursor, 3);
  assert.equal(JSON.stringify(batch).includes('private-secret'), false);
  assert.deepEqual(logs.since(3).entries, []);
});

test('diagnostic endpoint uses control auth and keeps bounded cursors', async t => {
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0 });
  t.after(() => engine.close());
  const batch = await request(engine, '/control/logs?since=0');
  assert.equal(batch.status, 200);
  assert.ok(batch.body.entries.some(entry => entry.code === 'engine_ready'));
  assert.equal((await fetch(engine.url + '/control/logs')).status, 401);
  assert.equal((await request(engine, '/control/logs?since=-1')).status, 400);
});
