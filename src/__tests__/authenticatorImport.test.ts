import { expect, it } from 'vitest'
import { createProfile } from '../domain'
import { applyAuthenticatorSecret, readAuthenticatorFile } from '../authenticatorImport'

it('selects only the chosen seed and preserves connection identity and unsaved edits', () => {
  const profile = { ...createProfile(), name: 'draft', username: 'alice', password: 'synthetic-only', tryKeyboard: false }
  const bound = applyAuthenticatorSecret(profile, 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ')
  expect(bound.id).toBe(profile.id)
  expect(bound.username).toBe('alice')
  expect(bound.password).toBe('synthetic-only')
  expect(bound.tryKeyboard).toBe(true)
  expect(bound.hasTotpSecret).toBe(true)
  expect(profile.totpSecret).toBeUndefined()
})

it('refuses invalid or oversized imports without exposing the source', async () => {
  expect(() => applyAuthenticatorSecret(createProfile(), 'synthetic-invalid-secret')).toThrow()
  await expect(readAuthenticatorFile(new File(['backup'], 'wrong.png'))).rejects.toThrow('JSON 或 TXT')
  await expect(readAuthenticatorFile(new File(['x'.repeat(1024 * 1024 + 1)], 'large.json'))).rejects.toThrow('1 MB')
  expect(await readAuthenticatorFile(new File(['otpauth://totp/test'], 'authenticator.txt'))).toBe('otpauth://totp/test')
})
