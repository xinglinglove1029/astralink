import test from 'node:test';
import assert from 'node:assert/strict';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { feature, profile, tokens, request, fakeSsh, eventually } from './helpers.mjs';
async function start(t, options = {}) {
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0, ...options });
  t.after(() => engine.close());
  const client = new Client({ name: 'review regression', version: '1' });
  await client.connect(new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokens.mcpToken } } }));
  t.after(() => client.close());
  return { engine, client };
}
test('retired operation credentials remain redacted through remote channel rejection result', async (t) => {
  const password = 'retired-password-sentinel', passphrase = 'retired-passphrase-sentinel';
  let rejectChannel;
  const ssh = await fakeSsh({ password, channelRejection: 'denied ' + password + ' ' + passphrase, onSession: ({ reject }) => {
      rejectChannel = reject;
    } });
  t.after(() => ssh.close());
  const { engine, client } = await start(t);
  const p = profile({ password, passphrase, tryKeyboard: false, port: ssh.port });
  await request(engine, '/control/profiles', { profiles: [p] });
  const operation = client.callTool({ name: 'execute-command', arguments: { connectionName: p.name, cmdString: 'hostname' } });
  await eventually(() => rejectChannel);
  const oldEntry = engine.pool.entries.get(p.name);
  await request(engine, '/control/profiles', { profiles: [{ ...p, password: 'replacement-password', passphrase: 'replacement-passphrase' }] });
  assert.equal(oldEntry.retired, true);
  rejectChannel();
  const result = await operation;
  assert.equal(result.isError, true);
  assert.equal(engine.pool.retired.size, 0);
  assert.ok(!engine.pool.secrets().includes(password));
  const output = JSON.stringify(result);
  assert.ok(!output.includes(password), 'Retired password leaked in tool result');
  assert.ok(!output.includes(passphrase), 'Retired passphrase leaked in tool result');
  assert.match(output, /redacted/);
});
test('omitted connectionName requires explicit enabled default and never falls back to production', async (t) => {
  const ssh = await fakeSsh({ password: 'default-test-pass' });
  t.after(() => ssh.close());
  const { engine, client } = await start(t);
  const production = profile({ name: 'production', username: 'production-user', port: ssh.port, password: 'default-test-pass', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [production] });
  const omitted = () => client.callTool({ name: 'execute-command', arguments: { cmdString: 'hostname' } });
  const denied = await omitted();
  assert.equal(denied.isError, true);
  assert.match(denied.content[0].text, /连接不存在/);
  assert.equal(ssh.seen.connections, 0);
  assert.deepEqual(ssh.seen.commands, []);
  const empty = await client.callTool({ name: 'execute-command', arguments: { cmdString: 'hostname', connectionName: '' } });
  assert.equal(empty.isError, true);
  assert.equal(ssh.seen.connections, 0);
  const explicit = await client.callTool({ name: 'execute-command', arguments: { cmdString: 'hostname', connectionName: production.name } });
  assert.ok(!explicit.isError);
  const defaultProfile = profile({ id: '22222222-2222-4222-8222-222222222222', name: 'default', username: 'default-user', port: ssh.port, password: 'default-test-pass', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [production, defaultProfile] });
  const selected = await omitted();
  assert.ok(!selected.isError);
  assert.equal(ssh.seen.usernames.at(-1), 'default-user');
  await request(engine, '/control/profiles', { profiles: [production, { ...defaultProfile, enabled: false }] });
  const commandsBefore = ssh.seen.commands.length;
  assert.equal((await omitted()).isError, true);
  assert.equal(ssh.seen.commands.length, commandsBefore);
});
test('one-time password prompt reaches UI instead of sending the stored password', async (t) => {
  const ssh = await fakeSsh({ prompts: [{ prompt: 'One-time password: ', echo: false }] });
  t.after(() => ssh.close());
  const { engine } = await start(t, { authOptions: { ttlMs: 2000 } });
  const p = profile({ port: ssh.port, password: 'stored-password-must-not-be-otp' });
  await request(engine, '/control/profiles', { profiles: [p] });
  let completed;
  const probe = request(engine, '/control/test', { profileId: p.id }).then(result => {
    completed = result;
    return result;
  });
  await eventually(() => engine.auth.pending.size || completed);
  assert.equal(engine.auth.pending.size, 1, 'OTP was auto-filled instead of prompting UI');
  assert.deepEqual(ssh.seen.responses, []);
  const event = (await request(engine, '/control/events')).body.events[0];
  assert.equal(event.prompts[0].prompt, 'One-time password: ');
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  assert.equal((await probe).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['654321']]);
});
test('ambiguous password prompt reaches UI and never autofills a stored credential', async (t) => {
  const ssh = await fakeSsh({ prompts: [{ prompt: 'Account or token password: ', echo: false }] });
  t.after(() => ssh.close());
  const { engine } = await start(t, { authOptions: { ttlMs: 2000 } });
  const p = profile({ port: ssh.port, password: 'stored-ambiguous-password' });
  await request(engine, '/control/profiles', { profiles: [p] });
  const probe = request(engine, '/control/test', { profileId: p.id });
  const event = await eventually(() => engine.auth.events(0).events[0]);
  assert.deepEqual(ssh.seen.responses, []);
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  assert.equal((await probe).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['654321']]);
});
test('unknown password-labelled prompt reaches UI rather than matching a password suffix', async (t) => {
  const ssh = await fakeSsh({ prompts: [{ prompt: 'Enter account password: ', echo: false }] });
  t.after(() => ssh.close());
  const { engine } = await start(t, { authOptions: { ttlMs: 2000 } });
  const p = profile({ port: ssh.port, password: 'stored-unknown-password' });
  await request(engine, '/control/profiles', { profiles: [p] });
  let completed;
  const probe = request(engine, '/control/test', { profileId: p.id }).then(result => {
    completed = result;
    return result;
  });
  await eventually(() => engine.auth.pending.size || completed);
  assert.equal(engine.auth.pending.size, 1);
  assert.deepEqual(ssh.seen.responses, []);
  const event = engine.auth.events(0).events[0];
  await request(engine, '/control/auth', { requestId: event.requestId, code: '654321' });
  assert.equal((await probe).body.status, 'connected');
  assert.deepEqual(ssh.seen.responses, [['654321']]);
});
test('complete connection deadline retires a stalled forward and destroys its late stream without dropping jump', async (t) => {
  let forwarding;
  const jump = await fakeSsh({ password: 'jump-timeout-pass', allowForwarding: true, onForward: request => {
      forwarding = request;
    } });
  t.after(() => jump.close());
  const { engine } = await start(t, { poolOptions: { connectionTimeoutMs: 350 } });
  const gateway = profile({ name: 'gateway', port: jump.port, password: 'jump-timeout-pass', tryKeyboard: false });
  const asset = profile({ id: '22222222-2222-4222-8222-222222222222', name: 'asset', port: 3333, password: 'target-pass', tryKeyboard: false, jumpHostId: gateway.id });
  await request(engine, '/control/profiles', { profiles: [gateway, asset] });
  assert.equal((await request(engine, '/control/test', { profileId: gateway.id })).body.status, 'connected');
  const gatewayEntry = engine.pool.entries.get(gateway.name), assetEntry = engine.pool.entries.get(asset.name);
  let completed, probeError;
  const began = Date.now();
  const probe = request(engine, '/control/test', { profileId: asset.id }).then(result => {
    completed = result;
    return result;
  }, error => {
    probeError = error;
  });
  await eventually(() => forwarding);
  assert.equal(gatewayEntry.active, 1);
  await request(engine, '/control/profiles', { profiles: [gateway] });
  assert.equal(assetEntry.retired, true);
  await eventually(() => completed || probeError, 1200);
  assert.equal(probeError, undefined);
  assert.equal((await probe).body.status, 'failed');
  assert.ok(Date.now() - began < 1200);
  assert.equal(engine.pool.retired.size, 0);
  assert.equal(assetEntry.active, 0);
  assert.equal(assetEntry.manager.sockets.size, 0);
  assert.equal(gatewayEntry.active, 0);
  assert.equal(jump.seen.disconnected, 0);
  let lateClosed = false, bytesReceived = 0;
  const late = forwarding.accept();
  late.on('close', () => {
    lateClosed = true;
  });
  late.on('data', chunk => {
    bytesReceived += chunk.length;
  });
  await eventually(() => lateClosed);
  assert.equal(bytesReceived, 0);
  assert.equal(engine.pool.entries.get(gateway.name), gatewayEntry);
  assert.equal((await request(engine, '/control/test', { profileId: gateway.id })).body.status, 'connected');
});
test('connection deadline includes jump authentication and releases both retired generations', async (t) => {
  const jump = await fakeSsh();
  t.after(() => jump.close());
  const { engine } = await start(t, { poolOptions: { connectionTimeoutMs: 300 }, authOptions: { ttlMs: 5000 } });
  const gateway = profile({ name: 'gateway', port: jump.port });
  const asset = profile({ id: '22222222-2222-4222-8222-222222222222', name: 'asset', jumpHostId: gateway.id, port: 3333, password: 'target-pass', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [gateway, asset] });
  const gatewayEntry = engine.pool.entries.get(gateway.name), assetEntry = engine.pool.entries.get(asset.name);
  let completed;
  const began = Date.now();
  const probe = request(engine, '/control/test', { profileId: asset.id }).then(result => {
    completed = result;
    return result;
  });
  const event = await eventually(() => engine.auth.events(0).events[0]);
  assert.equal(event.profileId, gateway.id);
  await request(engine, '/control/profiles', { profiles: [] });
  assert.equal(gatewayEntry.retired, true);
  assert.equal(assetEntry.retired, true);
  await eventually(() => completed, 1000);
  assert.equal((await probe).body.status, 'failed');
  assert.ok(Date.now() - began < 1000);
  await eventually(() => engine.auth.pending.size === 0 && gatewayEntry.manager.pendingConnections.size === 0);
  assert.equal(engine.pool.retired.size, 0);
  assert.equal(assetEntry.active, 0);
  assert.equal(gatewayEntry.active, 0);
  assert.equal(assetEntry.manager.sockets.size, 0);
  assert.equal(gatewayEntry.manager.sockets.size, 0);
});
test('late rejected forward after timeout releases its lease once and preserves the jump connection', async (t) => {
  let forwarding;
  const jump = await fakeSsh({ password: 'late-error-pass', allowForwarding: true, onForward: request => {
      forwarding = request;
    } });
  t.after(() => jump.close());
  const { engine } = await start(t, { poolOptions: { connectionTimeoutMs: 300 } });
  const gateway = profile({ name: 'gateway', port: jump.port, password: 'late-error-pass', tryKeyboard: false });
  const asset = profile({ id: '22222222-2222-4222-8222-222222222222', name: 'asset', jumpHostId: gateway.id, port: 3333, password: 'target-pass', tryKeyboard: false });
  await request(engine, '/control/profiles', { profiles: [gateway, asset] });
  assert.equal((await request(engine, '/control/test', { profileId: gateway.id })).body.status, 'connected');
  const gatewayEntry = engine.pool.entries.get(gateway.name);
  let completed;
  const probe = request(engine, '/control/test', { profileId: asset.id }).then(result => {
    completed = result;
    return result;
  });
  await eventually(() => forwarding);
  await eventually(() => completed, 1000);
  assert.equal((await probe).body.status, 'failed');
  assert.equal(gatewayEntry.active, 0);
  forwarding.reject();
  assert.equal((await request(engine, '/control/test', { profileId: gateway.id })).body.status, 'connected');
  assert.equal(gatewayEntry.active, 0);
  assert.equal(jump.seen.disconnected, 0);
});
