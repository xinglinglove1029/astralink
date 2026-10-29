import test from 'node:test';
import assert from 'node:assert/strict';
import { feature } from './helpers.mjs';

test('automatic OTP waits through rollover and allocates distinct steps across connections sharing a seed', async () => {
  const { TotpScheduler } = await feature('totp-scheduler');
  const { totp } = await feature('totp');
  let now = 58_000;
  const waits = [];
  const scheduler = new TotpScheduler({ now: () => now, sleep: async ms => { waits.push(ms); now += ms; } });
  const secret = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  const results = await Promise.all([scheduler.next(secret), scheduler.next(secret), scheduler.next(secret)]);
  assert.equal(new Set(results.map(result => result.step)).size, 3);
  assert.ok(results.every(result => result.code === totp(secret, result.step * 30000).code));
  assert.ok(waits.length >= 1);
  assert.ok(results.every(result => result.remainingMs >= 5000));
});

test('OTP waiting is cancellable and never submits after shutdown', async () => {
  const { TotpScheduler } = await feature('totp-scheduler');
  const abort = new AbortController();
  const scheduler = new TotpScheduler({ now: () => 58_000 });
  const pending = scheduler.next('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ', { signal: abort.signal });
  const rejected = assert.rejects(pending);
  abort.abort(new Error('cancelled'));
  await rejected;
  scheduler.close();
  await assert.rejects(scheduler.next('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'));
});

test('clock rollback cannot reuse a previously submitted OTP', async () => {
  const { TotpScheduler } = await feature('totp-scheduler');
  let now = 90050;
  const scheduler = new TotpScheduler({ now: () => now });
  await scheduler.next('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ');
  now = 60050;
  await assert.rejects(scheduler.next('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'), /时间回退/);
  scheduler.close();
});

test('RFC 6238 SHA-1 vectors produce six digit authenticator codes', async () => {
  const { totp } = await feature('totp');
  const secret = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  assert.equal(totp(secret, 59_000).code, '287082');
  assert.equal(totp(secret, 1_111_111_109_000).code, '081804');
});
