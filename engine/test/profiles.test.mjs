import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { feature, profile } from './helpers.mjs';
test('profile validation rejects duplicates, invalid ports, secret types and unresolved jump references', async () => {
  const { validateProfiles } = await feature('profiles');
  for (const profiles of [[profile(), profile()], [profile({ port: 0 })], [profile({ port: '22' })], [profile({ password: 123 })], [profile({ id: 'bad' })], [profile({ jumpHostId: '22222222-2222-4222-8222-222222222222' })], [profile({ targetHost: 'asset' })]])
    await assert.rejects(() => validateProfiles(profiles));
});
test('private key file must exist and be a usable key, including disabled profiles', async () => {
  const { validateProfiles } = await feature('profiles');
  await assert.rejects(() => validateProfiles([profile({ authType: 'privateKey', privateKeyPath: 'Z:/missing/key' })]));
  await assert.rejects(() => validateProfiles([profile({ enabled: false, authType: 'privateKey', privateKeyPath: 'Z:/missing/key' })]));
});
test('JumpServer direct login maps platform, account and asset explicitly', async () => {
  const { validateProfiles, toSshConfig } = await feature('profiles');
  const [p] = await validateProfiles([profile({ username: 'platform', targetHost: 'asset', targetUsername: 'root' })]);
  assert.equal(toSshConfig(p).username, 'platform@root@asset');
  assert.equal(toSshConfig(p).host, '127.0.0.1');
});
test('validation rejects cyclic jump relations and unsafe direct login delimiters', async () => {
  const { validateProfiles } = await feature('profiles');
  await assert.rejects(() => validateProfiles([profile({ jumpHostId: profile().id })]));
  await assert.rejects(() => validateProfiles([profile({ targetHost: 'asset', targetUsername: 'root@other' })]));
});
test('validates and retains a base32 TOTP seed only in runtime profiles', async () => {
  const { validateProfiles } = await feature('profiles');
  const seed = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const [p] = await validateProfiles([profile({ totpSecret: seed, hasTotpSecret: true })]);
  assert.equal(p.totpSecret, seed);
  await assert.rejects(() => validateProfiles([profile({ totpSecret: 'not base32!' })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ totpSecret: 'ABC', hasTotpSecret: true })]), { code: 'INVALID_PROFILE' });
});
test('path allowlists are bounded absolute paths and reject malformed input atomically', async () => {
  const { validateProfiles } = await feature('profiles');
  for (const field of ['allowedLocalPaths', 'allowedRemotePaths']) {
    for (const value of [null, 'directory', [12], [''], ['relative/path'], ['x'.repeat(4097)], ['/path\nwith-newline'], Array(33).fill('/bounded')]) {
      await assert.rejects(() => validateProfiles([profile({ [field]: value })]), { code: 'INVALID_PROFILE' }, field);
    }
  }
  await assert.rejects(() => validateProfiles([profile({ allowedLocalPaths: [path.join(process.cwd(), '*.zip')] })]));
  await assert.rejects(() => validateProfiles([profile({ allowedRemotePaths: ['C:relative'] })]));
  await assert.rejects(() => validateProfiles([profile({ allowedRemotePaths: ['//server/share'] })]));
});
test('validated runtime profiles retain independent frozen path allowlists and forward them to upstream', async () => {
  const { validateProfiles, toSshConfig } = await feature('profiles');
  const local = [path.join(process.cwd(), 'engine', 'test')];
  const remote = ['/deploy/app', 'C:/deploy/app'];
  const [p] = await validateProfiles([profile({ allowedLocalPaths: local, allowedRemotePaths: remote })]);
  assert.deepEqual(p.allowedLocalPaths, local);
  assert.deepEqual(p.allowedRemotePaths, remote);
  assert.deepEqual(toSshConfig(p).allowedLocalPaths, local);
  assert.deepEqual(toSshConfig(p).allowedRemotePaths, remote);
  local.push(path.parse(process.cwd()).root);
  remote.push('/');
  assert.equal(p.allowedLocalPaths.length, 1);
  assert.equal(p.allowedRemotePaths.length, 2);
  assert.ok(Object.isFrozen(p.allowedLocalPaths));
  assert.ok(Object.isFrozen(p.allowedRemotePaths));
  const [defaults] = await validateProfiles([profile()]);
  assert.equal(toSshConfig(defaults).allowedLocalPaths, undefined);
  assert.equal(toSshConfig(defaults).allowedRemotePaths, undefined);
});
test('notes preserve bounded LF, CRLF and tab text without loosening other profile fields', async () => {
  const { validateProfiles } = await feature('profiles');
  const lf = '第一行\n\t第二行\n';
  const crlf = '第一行\r\n\t第二行\r\n';
  const mixed = '第一行\n第二行\r\n\t第三行';
  assert.equal((await validateProfiles([profile({ notes: lf })]))[0].notes, lf);
  assert.equal((await validateProfiles([profile({ notes: crlf })]))[0].notes, crlf);
  assert.equal((await validateProfiles([profile({ notes: mixed })]))[0].notes, mixed);
  const boundary = 'a'.repeat(4094) + '\r\n';
  assert.equal((await validateProfiles([profile({ notes: boundary })]))[0].notes, boundary);
  assert.equal((await validateProfiles([profile({ notes: '' })]))[0].notes, '');
  const noNotes = profile();
  delete noNotes.notes;
  assert.equal((await validateProfiles([noNotes]))[0].notes, '');
  await assert.rejects(() => validateProfiles([profile({ name: 'bad\nname' })]), { code: 'INVALID_PROFILE' });
});
test('notes reject NUL, other controls, lone CR, non-string values and overlong text', async () => {
  const { validateProfiles } = await feature('profiles');
  for (let code = 0; code <= 0x1f; code++) {
    if (code === 9 || code === 10 || code === 13)
      continue;
    await assert.rejects(() => validateProfiles([profile({ notes: 'before' + String.fromCharCode(code) + 'after' })]), { code: 'INVALID_PROFILE' });
  }
  for (let code = 0x7f; code <= 0x9f; code++)
    await assert.rejects(() => validateProfiles([profile({ notes: 'before' + String.fromCharCode(code) + 'after' })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ notes: 'before\rafter' })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ notes: 'a'.repeat(4097) })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ notes: null })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ notes: 12 })]), { code: 'INVALID_PROFILE' });
  await assert.rejects(() => validateProfiles([profile({ notes: [] })]), { code: 'INVALID_PROFILE' });
});
