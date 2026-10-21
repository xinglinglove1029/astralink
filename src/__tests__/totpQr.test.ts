import { describe, expect, it } from 'vitest'
import { extractTotpSecret, readTotpSecretFromImage } from '../totpQr'

describe('TOTP QR payloads', () => {
  it('extracts a Base32 seed from a TOTP provisioning URI', () => {
    expect(extractTotpSecret('otpauth://totp/JumpServer:ops?secret=gezdgnbvgy3tqojqgezdgnbvgy3tqojq&issuer=JumpServer'))
      .toBe('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ')
  })

  it('accepts a raw Base32 QR payload', () => {
    expect(extractTotpSecret('GEZD GNBV GY3T QOJQ GEZD GNBV GY3T QOJQ'))
      .toBe('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ')
  })

  it('rejects unsupported QR schemes and HOTP tokens', () => {
    expect(() => extractTotpSecret('https://example.test/?secret=GEZDGNBVGY3TQOJQ')).toThrow('不支持的二维码')
    expect(() => extractTotpSecret('otpauth://hotp/JumpServer?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ')).toThrow('不支持的二维码')
  })

  it('rejects missing, duplicate, and invalid seed parameters without disclosing payloads', () => {
    const invalid = 'otpauth://totp/JumpServer:ops?secret=private-secret&issuer=JumpServer'
    expect(() => extractTotpSecret('otpauth://totp/JumpServer:ops?issuer=JumpServer')).toThrow('二维码中没有有效的动态密钥')
    expect(() => extractTotpSecret('otpauth://totp/JumpServer:ops?secret=private-secret&secret=other-secret')).toThrow('二维码中没有有效的动态密钥')
    try { extractTotpSecret(invalid) } catch (error) { expect(String(error)).not.toContain('private-secret') }
  })

  it('rejects non-image files before attempting local decoding', async () => {
    const file = new File(['not an image'], 'credential.txt', { type: 'text/plain' })
    await expect(readTotpSecretFromImage(file)).rejects.toThrow('请选择 8 MB 以内的二维码图片')
  })
})
