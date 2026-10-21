import test from 'node:test';
import assert from 'node:assert/strict';
import { feature } from './helpers.mjs';

test('RFC 6238 SHA-1 vectors produce six digit authenticator codes', async () => {
  const { totp } = await feature('totp');
  const secret = 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ';
  assert.equal(totp(secret, 59_000).code, '287082');
  assert.equal(totp(secret, 1_111_111_109_000).code, '081804');
});
