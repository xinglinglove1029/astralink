import type { ConnectionProfile } from './types'
import { extractTotpSecret } from './totpQr'

export async function readAuthenticatorFile(file: File): Promise<string> {
  if (!/\.(json|txt)$/i.test(file.name)) throw new Error('请选择 JSON 或 TXT 认证器备份')
  if (!file.size || file.size > 1024 * 1024) throw new Error('备份文件需在 1 MB 以内')
  return file.text()
}

export function applyAuthenticatorSecret(profile: ConnectionProfile, secret: string): ConnectionProfile {
  return { ...profile, totpSecret: extractTotpSecret(secret), hasTotpSecret: true, tryKeyboard: true }
}
