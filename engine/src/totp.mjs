import { createHmac } from 'node:crypto';

export function normalizeTotpSecret(value) {
  if (typeof value !== 'string')
    throw new TypeError('Invalid TOTP secret');
  const secret = value.replace(/\s/g, '').replace(/=+$/, '').toUpperCase();
  if (!/^[A-Z2-7]{16,128}$/.test(secret) || ![0, 2, 4, 5, 7].includes(secret.length % 8))
    throw new TypeError('Invalid TOTP secret');
  return secret;
}

function decodeBase32(secret) {
  let bits = '';
  for (const character of normalizeTotpSecret(secret))
    bits += 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(character).toString(2).padStart(5, '0');
  const bytes = [];
  for (let offset = 0; offset + 8 <= bits.length; offset += 8)
    bytes.push(Number.parseInt(bits.slice(offset, offset + 8), 2));
  if (bits.slice(bytes.length * 8).includes('1'))
    throw new TypeError('Invalid TOTP secret');
  return Buffer.from(bytes);
}

export function totp(secret, now = Date.now()) {
  const step = Math.floor(now / 30_000);
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const digest = createHmac('sha1', decodeBase32(secret)).update(counter).digest();
  const offset = digest[digest.length - 1] & 0x0f;
  const binary = ((digest[offset] & 0x7f) << 24) | (digest[offset + 1] << 16) | (digest[offset + 2] << 8) | digest[offset + 3];
  return { code: String(binary % 1_000_000).padStart(6, '0'), step, remainingMs: 30_000 - now % 30_000 };
}
