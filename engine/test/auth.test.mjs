import test from 'node:test';
import assert from 'node:assert/strict';
import { feature, profile } from './helpers.mjs';
test('OTP queue correlates responses, rejects replays, and never retains a code in events', async () => {
  const { AuthQueue } = await feature('auth');
  const queue = new AuthQueue({ ttlMs: 1000, maxPending: 2, maxHistory: 2 });
  const first = queue.challenge(profile(), 'Code required', [{ prompt: 'OTP:', echo: false }]);
  const second = queue.challenge(profile({ id: '22222222-2222-4222-8222-222222222222' }), '', [{ prompt: 'OTP:', echo: false }]);
  const events = queue.events(0);
  assert.equal(events.events.length, 2);
  assert.equal(queue.respond({ requestId: events.events[1].requestId, code: 'two' }), true);
  assert.deepEqual(await second, ['two']);
  assert.equal(queue.respond({ requestId: events.events[1].requestId, code: 'replay' }), false);
  assert.equal(queue.respond({ requestId: events.events[0].requestId, code: 'one' }), true);
  assert.deepEqual(await first, ['one']);
  assert.equal(JSON.stringify(queue.events(0)).includes('two'), false);
  queue.close();
});
test('OTP pending limit, cancel, expiry, profile retirement and shutdown settle waiters', async () => {
  const { AuthQueue } = await feature('auth');
  const queue = new AuthQueue({ ttlMs: 35, maxPending: 1, maxHistory: 2 });
  const first = queue.challenge(profile(), '', [{ prompt: 'OTP', echo: false }]);
  const rejected = assert.rejects(first, { code: 'AUTH_CANCELLED' });
  await assert.rejects(queue.challenge(profile(), '', [{ prompt: 'OTP', echo: false }]), { code: 'AUTH_QUEUE_FULL' });
  assert.equal(queue.respond({ requestId: queue.events(0).events[0].requestId, cancel: true }), true);
  await rejected;
  await assert.rejects(queue.challenge(profile(), '', [{ prompt: 'OTP', echo: false }]), { code: 'AUTH_EXPIRED' });
  const third = queue.challenge(profile(), '', [{ prompt: 'OTP', echo: false }]);
  const shutdown = assert.rejects(third, { code: 'AUTH_CANCELLED' });
  queue.close();
  await shutdown;
  assert.ok(queue.events(0).events.length <= 2);
});
test('challenge text is bounded and secrets in server prompts are redacted', async () => {
  const { AuthQueue } = await feature('auth');
  const queue = new AuthQueue();
  const p = profile({ password: 'never-show-password', passphrase: 'never-show-passphrase', totpSecret: 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ' });
  const promise = queue.challenge(p, p.password + 'x'.repeat(20000), [{ prompt: p.passphrase, echo: false }]);
  const settled = assert.rejects(promise);
  const snapshot = JSON.stringify(queue.events(0));
  assert.ok(snapshot.length < 10000);
  assert.ok(!snapshot.includes(p.password));
  assert.ok(!snapshot.includes(p.passphrase));
  assert.ok(!snapshot.includes(p.totpSecret));
  queue.close();
  await settled;
});
test('history churn never hides a still-pending challenge from an initial poll', async (t) => {
  const { AuthQueue } = await feature('auth');
  const queue = new AuthQueue({ maxPending: 2, maxHistory: 2 });
  const first = queue.challenge(profile(), '', [{ prompt: 'old pending', echo: false }]);
  const firstRejected = assert.rejects(first);
  t.after(async () => {
    queue.close();
    await firstRejected;
  });
  for (let i = 0; i < 4; i++) {
    const next = queue.challenge(profile(), '', [{ prompt: 'new', echo: false }]);
    const rejected = assert.rejects(next);
    const current = queue.events(0).events.at(-1);
    queue.respond({ requestId: current.requestId, cancel: true });
    await rejected;
  }
  assert.equal(queue.events(0).events.length, 1);
  assert.equal(queue.events(0).events[0].prompts[0].prompt, 'old pending');
  queue.close();
  await firstRejected;
});
