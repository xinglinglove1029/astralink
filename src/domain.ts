import type { AppSettings, ConnectionProfile, ConnectionTest, SourceKind } from './types'

export function connectionResultLabel(status: ConnectionTest['status']): string {
  switch (status) {
    case 'connected': return '测试通过'
    case 'partial': return '堡垒机已登录'
    case 'auth_required': return '认证未完成'
    case 'failed': return '测试失败'
  }
}

export const sourceLabels: Record<SourceKind, string> = { xshell: 'Xshell', tabby: 'Tabby', netcatty: 'Netcatty', 'mcp-json': 'MCP JSON', openssh: 'OpenSSH', finalshell: 'FinalShell', nyaterm: 'NyaTerm', mobaxterm: 'MobaXterm', securecrt: 'SecureCRT', electerm: 'Electerm', windterm: 'WindTerm', termius: 'Termius', 'nya-backup': 'NyaTerm 备份' }
export const authLabels = { password: '密码', privateKey: '私钥', agent: 'SSH Agent' }
function profileId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6]! & 0x0f) | 0x40
  bytes[8] = (bytes[8]! & 0x3f) | 0x80
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
export function createProfile(): ConnectionProfile {
  return { id: profileId(), name: '', host: '', port: 22, username: '', group: '', source: 'manual', authType: 'password', hasPassword: false, hasPassphrase: false, hasTotpSecret: false, transportMode: 'exec', tryKeyboard: false, enabled: true, notes: '' }
}
export function needsCredential(profile: ConnectionProfile) {
  return profile.authType === 'password' && !profile.hasPassword && !profile.password && !profile.tryKeyboard
    || profile.authType === 'privateKey' && !profile.privateKeyPath?.trim()
}
export function validateProfile(profile: ConnectionProfile, existing: ConnectionProfile[], credentialRequired = true): Record<string, string> {
  const errors: Record<string, string> = {}
  if (!profile.name.trim()) errors.name = '请输入名称'
  else if (existing.some((item) => item.id !== profile.id && item.name.trim().toLocaleLowerCase() === profile.name.trim().toLocaleLowerCase())) errors.name = '名称已存在，请使用其他名称'
  if (!profile.host.trim() || /\s|:\/\//.test(profile.host)) errors.host = '请输入主机名或 IP，不含协议和空格'
  if (!profile.username.trim()) errors.username = '请输入用户名'
  if (!Number.isInteger(profile.port) || profile.port < 1 || profile.port > 65535) errors.port = '端口范围为 1–65535'
  if (credentialRequired && profile.authType === 'password' && needsCredential(profile)) errors.password = '请输入密码，或启用交互认证'
  if (credentialRequired && profile.authType === 'privateKey' && !profile.privateKeyPath?.trim()) errors.privateKeyPath = '请选择私钥文件'
  if (credentialRequired && profile.authType === 'agent' && !profile.agent?.trim()) errors.agent = '请输入 Agent 路径'
  if (profile.totpSecret?.trim()) {
    const seed = profile.totpSecret.replace(/\s/g, '').replace(/=+$/, '').toUpperCase()
    if (!/^[A-Z2-7]{16,128}$/.test(seed) || ![0, 2, 4, 5, 7].includes(seed.length % 8)) errors.totpSecret = '请输入有效的 Base32 动态密钥'
  }
  for (const key of ['allowedLocalPaths', 'allowedRemotePaths'] as const) {
    if (profile[key]?.some((path) => !/^(?:[A-Za-z]:[\\/]|\/|\\\\)/.test(path.trim()) || /[\x00-\x1f\x7f]/.test(path))) errors[key] = '请填写绝对路径，每行一个目录'
  }
  if (profile.jumpHostId && !existing.some((item) => item.id === profile.jumpHostId && item.id !== profile.id)) errors.jumpHostId = '跳板机不存在，或指向当前连接'
  if (profile.targetHost?.trim() && !profile.targetUsername?.trim()) errors.targetUsername = '请填写目标账号'
  if (profile.targetUsername?.trim() && !profile.targetHost?.trim()) errors.targetHost = '请填写目标资产'
  return errors
}
export function normalizeProfile(profile: ConnectionProfile): ConnectionProfile {
  const result = { ...profile, name: profile.name.trim(), host: profile.host.trim(), username: profile.username.trim(), group: profile.group.trim(), notes: profile.notes.trim() }
  for (const key of ['privateKeyPath', 'agent', 'jumpHostId', 'targetHost', 'targetUsername'] as const) result[key] = profile[key]?.trim() || undefined
  for (const key of ['allowedLocalPaths', 'allowedRemotePaths'] as const) result[key] = [...new Set(profile[key]?.map((path) => path.trim()).filter(Boolean) ?? [])]
  if (result.authType !== 'password' || !result.password) delete result.password
  if (result.authType !== 'privateKey' || !result.passphrase) delete result.passphrase
  if (result.totpSecret?.trim()) result.totpSecret = result.totpSecret.replace(/\s/g, '').replace(/=+$/, '').toUpperCase()
  else if (result.totpSecret === undefined) delete result.totpSecret
  return result
}
export function stripSecrets(profile: ConnectionProfile): ConnectionProfile {
  const { password: _password, passphrase: _passphrase, totpSecret: _totpSecret, ...metadata } = profile
  return metadata
}
export function inspectImport(profiles: ConnectionProfile[], existing: ConnectionProfile[]) {
  const ids = new Set<string>()
  const names = new Set<string>()
  const errors: string[] = []
  const conflicts: ConnectionProfile[] = []
  const needs = profiles.filter(needsCredential)
  for (const profile of profiles) {
    const name = profile.name.trim().toLocaleLowerCase()
    if (ids.has(profile.id) || names.has(name)) errors.push(`导入文件中的连接重复：${profile.name}`)
    ids.add(profile.id); names.add(name)
    const validation = validateProfile(profile, [...existing.filter((item) => !profiles.some((entry) => entry.id === item.id)), ...profiles], false)
    // Names that already exist are handled as conflicts by the chosen import strategy.
    delete validation.name
    if (!profile.name.trim()) validation.name = '请输入名称'
    for (const message of Object.values(validation)) errors.push(`${profile.name || '未命名'}：${message}`)
    if (existing.some((item) => item.id === profile.id || item.name.trim().toLocaleLowerCase() === name)) conflicts.push(profile)
  }
  return { errors: [...new Set(errors)], conflicts, needsCredential: needs }
}
export function validateSettings(settings: AppSettings) {
  return Number.isInteger(settings.mcpPort) && settings.mcpPort >= 1024 && settings.mcpPort <= 65535 ? '' : 'MCP 端口范围为 1024–65535'
}
export function filterProfiles(profiles: ConnectionProfile[], query: string, group: string, source: string, auth: string) {
  const needle = query.trim().toLocaleLowerCase()
  return profiles.filter((profile) => (!group || profile.group === group)
    && (!source || profile.source === source) && (!auth || profile.authType === auth)
    && (!needle || [profile.name, profile.host, profile.username, profile.group, profile.notes].some((field) => field.toLocaleLowerCase().includes(needle))))
}
