import { createHash, timingSafeEqual } from 'node:crypto';
export function tokenMatches(header, token) {
  const supplied = typeof header === 'string' && /^Bearer [^\s]+$/.test(header) ? header.slice(7) : '';
  const expected = createHash('sha256').update(token).digest();
  const actual = createHash('sha256').update(supplied).digest();
  return timingSafeEqual(expected, actual) && supplied.length > 0;
}
export function loopbackAuthority(authority) {
  return /^(?:127\.0\.0\.1|localhost|\[::1\])(?::[0-9]{1,5})?$/.test(authority);
}
export function validOrigin(origin) {
  if (origin === undefined)
    return true;
  if (typeof origin !== 'string')
    return false;
  try {
    const url = new URL(origin);
    return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password && url.origin === origin && loopbackAuthority(url.host);
  }
  catch {
    return false;
  }
}
