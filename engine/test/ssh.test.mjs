import test from 'node:test';
import assert from 'node:assert/strict';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { feature, profile, tokens, request, fakeSsh, eventually } from './helpers.mjs';
async function setup(t, serverOptions = {}, engineOptions = {}) {
  const ssh = await fakeSsh(serverOptions);
  t.after(() => ssh.close());
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0, ...engineOptions });
  t.after(() => engine.close());
  const p = profile({ port: ssh.port, ...(serverOptions.password ? { password: serverOptions.password, hasPassword: true, tryKeyboard: false } : {}) });
  await request(engine, '/control/profiles', { profiles: [p] });
  return { ssh, engine, p };
}
test('actual ssh2 keyboard-interactive challenge waits for control OTP and executes readonly probe', async (t) => {
  const { ssh, engine, p } = await setup(t);
  const pending = request(engine, '/control/test', { profileId: p.id });
  const events = await eventually(async () => {
    const result = await request(engine, '/control/events?since=0');
    return result.body.events.length && result.body;
  });
  const challenge = events.events[0];
  assert.equal(challenge.profileId, p.id);
  assert.equal(challenge.prompts[0].echo, false);
  assert.equal(challenge.host, '127.0.0.1');
  assert.equal((await request(engine, '/control/auth', { requestId: challenge.requestId, code: '654321' })).body.accepted, true);
  const result = await pending;
  assert.equal(result.body.status, 'connected');
  assert.match(result.body.stdout, /fake-host/);
  assert.deepEqual(ssh.seen.commands, ['hostname && id']);
  assert.deepEqual(ssh.seen.responses, [['654321']]);
  assert.equal((await request(engine, '/control/auth', { requestId: challenge.requestId, code: '654321' })).body.accepted, false);
});
test('real OTP cancellation and expiry finish SSH probes without hanging', async (t) => {
  const { engine, p } = await setup(t, {}, { authOptions: { ttlMs: 100, maxPending: 2, maxHistory: 4 } });
  const cancelled = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => {
    const result = await request(engine, '/control/events');
    return result.body.events[0];
  });
  await request(engine, '/control/auth', { requestId: event.requestId, cancel: true });
  const cancelledResult = (await cancelled).body;
  assert.equal(cancelledResult.status, 'auth_required');
  assert.match(cancelledResult.detail, /取消/);
  const expired = await request(engine, '/control/test', { profileId: p.id });
  assert.equal(expired.body.status, 'auth_required');
  assert.equal(expired.body.stage, 'authentication');
  assert.match(expired.body.detail, /超时.*重新测试/);
});
test('upstream MCP execute-command uses the OTP bridge and preserves nonzero exit output', async (t) => {
  const { engine, ssh } = await setup(t);
  const client = new Client({ name: 'SSH test', version: '1.0' });
  t.after(() => client.close());
  await client.connect(new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokens.mcpToken } } }));
  const called = client.callTool({ name: 'execute-command', arguments: { connectionName: 'test', cmdString: 'nonzero' } });
  const event = await eventually(async () => {
    const result = await request(engine, '/control/events');
    return result.body.events[0];
  });
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  const result = await called;
  assert.equal(result.isError, undefined);
  assert.match(result.content[0].text, /\[exit code\] 7/);
  assert.deepEqual(ssh.seen.responses, [['654321']]);
});
test('incremental updates preserve an unrelated live connection and drain modified active commands', async (t) => {
  let held;
  const { engine, ssh, p } = await setup(t, { password: 'test-secret', onHold: stream => {
      held = stream;
    } });
  const initial = await request(engine, '/control/test', { profileId: p.id });
  assert.equal(initial.body.status, 'connected');
  const original = engine.pool.entries.get(p.name);
  const second = profile({ id: '22222222-2222-4222-8222-222222222222', name: 'other', port: ssh.port, password: 'test-secret', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [p, second] });
  assert.equal(engine.pool.entries.get(p.name), original);
  assert.equal(ssh.seen.connections, 1);
  const command = engine.pool.executeCommand('hold', undefined, p.name);
  await eventually(() => held);
  await request(engine, '/control/profiles', { profiles: [{ ...p, username: 'new-user' }, second] });
  assert.equal(ssh.seen.disconnected, 0);
  held.write('completed');
  held.exit(0);
  held.end();
  assert.equal(await command, 'completed');
  await eventually(() => ssh.seen.disconnected === 1);
  assert.equal((await request(engine, '/control/test', { profileId: second.id })).body.status, 'connected');
  await request(engine, '/control/profiles', { profiles: [{ ...p, username: 'new-user' }, second] });
  assert.equal(ssh.seen.connections, 2);
});
test('mixed password and OTP prompts use stored password only for actual password prompt', async (t) => {
  const { engine, p, ssh } = await setup(t, { prompts: [{ prompt: 'Password:', echo: false }, { prompt: 'OTP:', echo: false }] });
  await request(engine, '/control/profiles', { profiles: [{ ...p, password: 'stored-password' }] });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => {
    const result = await request(engine, '/control/events');
    return result.body.events[0];
  });
  assert.deepEqual(event.prompts, [{ prompt: 'OTP:', echo: false }]);
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  assert.equal((await pending).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['stored-password', '654321']]);
});
test('recognized TOTP prompts are answered automatically without exposing a challenge', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const fixedNow = 1_700_000_010_000;
  const { totp } = await feature('totp');
  const expected = totp(seed, fixedNow).code;
  const { engine, p, ssh } = await setup(t, { code: expected }, { poolOptions: { now: () => fixedNow } });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  assert.equal((await request(engine, '/control/test', { profileId: p.id })).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [[expected]]);
  assert.equal((await request(engine, '/control/events')).body.events.length, 0);
});
test('explicit JumpServer OTP prompt auto-answers even when the server sets echo=true', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const fixedNow = 1_700_000_010_000;
  const { totp } = await feature('totp');
  const expected = totp(seed, fixedNow).code;
  const { engine, p, ssh } = await setup(t, { code: expected, prompts: [{ prompt: '[OTP Code]: ', echo: true }] }, { poolOptions: { now: () => fixedNow } });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  let outcome;
  const pending = request(engine, '/control/test', { profileId: p.id }).then(result => { outcome = result.body; });
  const first = await eventually(async () => {
    const event = (await request(engine, '/control/events')).body.events[0];
    return event ? { event } : outcome ? { outcome } : null;
  });
  if (first.event) await request(engine, '/control/auth', { requestId: first.event.requestId, cancel: true });
  await pending;
  assert.equal(first.event, undefined);
  assert.equal(outcome.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [[expected]]);
  const logs = engine.diagnostics.since(0).entries;
  assert.ok(logs.some(entry => entry.code === 'auth_otp_echo'));
  assert.ok(logs.some(entry => entry.code === 'otp_auto'));
  assert.ok(logs.some(entry => entry.code === 'ssh_authenticated'));
  const serialized = JSON.stringify(logs);
  assert.equal(serialized.includes(seed), false);
  assert.equal(serialized.includes(expected), false);
  assert.equal(serialized.includes('[OTP Code]'), false);
  const traceIds = new Set(logs.filter(entry => entry.profileId === p.id).map(entry => entry.requestId));
  assert.equal(traceIds.size, 1);
});
test('a TOTP timestep is submitted at most once per profile and unrecognized prompts stay manual', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const fixedNow = 1_700_000_010_000;
  const { totp } = await feature('totp');
  const expected = totp(seed, fixedNow).code;
  const { engine, p, ssh } = await setup(t, { code: expected }, { poolOptions: { now: () => fixedNow } });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  assert.equal((await request(engine, '/control/test', { profileId: p.id })).body.status, 'connected');
  await request(engine, '/control/profiles', { profiles: [{ ...p, username: 'tester2', totpSecret: seed, hasTotpSecret: true }] });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
  assert.equal(event.prompts[0].prompt, 'Verification code: ');
  await request(engine, '/control/auth', { requestId: event.requestId, code: expected });
  assert.equal((await pending).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [[expected], [expected]]);
});
test('unrecognized interactive prompts are never answered from a saved TOTP seed', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const { engine, p, ssh } = await setup(t, { code: 'manual-code', prompts: [{ prompt: 'Security response:', echo: false }] });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
  assert.equal(event.prompts[0].prompt, 'Security response:');
  await request(engine, '/control/auth', { requestId: event.requestId, code: 'manual-code' });
  assert.equal((await pending).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['manual-code']]);
});
test('echoed non-OTP prompts remain manual even with a saved TOTP seed', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const { engine, p, ssh } = await setup(t, { code: 'manual-response', prompts: [{ prompt: 'Security response:', echo: true }] });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const challenge = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
  assert.deepEqual(challenge.prompts, [{ prompt: 'Security response:', echo: true }]);
  assert.equal(engine.pool.usedTotpSteps.has(p.id), false);
  await request(engine, '/control/auth', { requestId: challenge.requestId, code: 'manual-response' });
  assert.equal((await pending).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['manual-response']]);
});
test('TOTP challenge within the final five seconds falls back to manual entry', async (t) => {
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const fixedNow = 1_700_000_036_000;
  const { engine, p, ssh } = await setup(t, { code: 'manual-code' }, { poolOptions: { now: () => fixedNow } });
  await request(engine, '/control/profiles', { profiles: [{ ...p, totpSecret: seed, hasTotpSecret: true }] });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
  await request(engine, '/control/auth', { requestId: event.requestId, code: 'manual-code' });
  assert.equal((await pending).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['manual-code']]);
  assert.ok(engine.diagnostics.since(0).entries.some(entry => entry.code === 'otp_near_expiry'));
  assert.equal(JSON.stringify(engine.diagnostics.since(0)).includes('manual-code'), false);
});

for (const closeMenu of [true, false]) {
  test('accepted OTP followed by JumpServer menu reports target selection instead of authentication failure (' + (closeMenu ? 'closed channel' : 'timeout') + ')', async (t) => {
    const { engine, p, ssh } = await setup(t, { onExec: stream => {
      stream.write('\x1b[32mJumpServer 开源堡垒机\x1b[0m\r\n选择目标资产\r\nOpt>');
      if (closeMenu) { stream.exit(0); stream.end(); }
    } });
    const pending = request(engine, '/control/test', { profileId: p.id });
    const event = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
    assert.equal((await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' })).body.accepted, true);
    const result = (await pending).body;
    assert.deepEqual(ssh.seen.responses, [['654321']]);
    assert.equal(result.status, 'partial');
    assert.equal(result.stage, 'target');
    assert.match(result.detail, /认证成功.*JumpServer.*目标资产.*目标账号/);
    assert.equal(result.stdout, undefined);
    assert.equal((await request(engine, '/control/events')).body.events.length, 0);
  });
}
test('post-authentication command rejection is reported separately from OTP failure', async (t) => {
  const { engine, p } = await setup(t, { onSession: ({ reject }) => reject() });
  const pending = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(async () => (await request(engine, '/control/events')).body.events[0]);
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  const result = (await pending).body;
  assert.equal(result.status, 'failed');
  assert.equal(result.stage, 'command');
  assert.match(result.detail, /认证成功.*命令/);
});
