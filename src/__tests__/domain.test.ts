import { describe, expect, it, vi } from 'vitest'
import { createProfile, validateProfile, inspectImport, validateSettings, connectionResultLabel } from '../domain'
import type { ConnectionProfile } from '../types'

describe('profile IDs', () => {
  it('creates unique v4 UUIDs without crypto.randomUUID on HTTP', () => {
    const cryptoWithRandomValues = globalThis.crypto
    vi.stubGlobal('crypto', { getRandomValues: cryptoWithRandomValues.getRandomValues.bind(cryptoWithRandomValues) })
    try {
      const ids = Array.from({ length: 16 }, () => createProfile().id)
      expect(new Set(ids).size).toBe(ids.length)
      ids.forEach((id) => expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/))
    } finally {
      vi.unstubAllGlobals()
    }
  })
})

describe('connection test result labels', () => {
  it('does not label a finished authentication result as waiting', () => {
    expect(connectionResultLabel('auth_required')).toBe('认证未完成')
    expect(connectionResultLabel('partial')).toBe('堡垒机已登录')
    expect(connectionResultLabel('connected')).toBe('测试通过')
    expect(connectionResultLabel('failed')).toBe('测试失败')
  })
})

describe('profile validation', () => {
  it('requires real credentials but permits keyboard-interactive password auth', () => {
    const profile = { ...createProfile(), name: '生产', host: 'server.local', username: 'ops' }
    expect(validateProfile(profile, []).password).toBeTruthy()
    profile.tryKeyboard = true
    expect(validateProfile(profile, [])).toEqual({})
  })
  it('rejects invalid ports, duplicate names and unresolved jump hosts', () => {
    const profile = { ...createProfile(), name: '生产', host: 'server.local', username: 'ops', tryKeyboard: true, port: 1.5, jumpHostId: 'missing' }
    const other = { ...profile, id: 'other', port: 22 }
    const errors = validateProfile(profile, [other])
    expect(errors.port).toBeTruthy()
    expect(errors.name).toBeTruthy()
    expect(errors.jumpHostId).toBeTruthy()
  })
  it('preserves whitespace in passwords and accepts stored credentials', () => {
    const profile: ConnectionProfile = { ...createProfile(), name: '生产', host: 'server.local', username: 'ops', password: '  pass  ' }
    expect(validateProfile(profile, [])).toEqual({})
    delete profile.password
    profile.hasPassword = true
    expect(validateProfile(profile, [])).toEqual({})
  })
  it('validates and normalizes TOTP seeds while keeping them out of profile metadata', async () => {
    const { normalizeProfile, stripSecrets } = await import('../domain')
    const profile: ConnectionProfile = { ...createProfile(), name: 'MFA', host: 'server.local', username: 'ops', tryKeyboard: true, totpSecret: ' gezd gnbv gy3t qojq gezd gnbv gy3t qojq ' }
    expect(validateProfile(profile, [])).toEqual({})
    expect(normalizeProfile(profile).totpSecret).toBe('GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ')
    expect(stripSecrets(profile)).not.toHaveProperty('totpSecret')
    expect(validateProfile({ ...profile, totpSecret: 'invalid' }, []).totpSecret).toBeTruthy()
  })
  it('requires a key path and paired target asset/account', () => {
    const profile = { ...createProfile(), name: '堡垒机', host: 'bastion', username: 'ops', authType: 'privateKey' as const, targetHost: 'asset' }
    expect(validateProfile(profile, []).privateKeyPath).toBeTruthy()
    expect(validateProfile(profile, []).targetUsername).toBeTruthy()
  })
  it('does not let missing credentials block import but rejects duplicates', () => {
    const first = { ...createProfile(), name: '生产', host: 'server.local', username: 'ops' }
    const second = { ...first, id: 'second' }
    const result = inspectImport([first, second], [])
    expect(result.needsCredential).toHaveLength(2)
    expect(result.errors.length).toBeGreaterThan(0)
  })
  it('reports conflicts against both existing IDs and names', () => {
    const first = { ...createProfile(), name: '生产', host: 'server.local', username: 'ops' }
    const imported = { ...first, id: 'imported' }
    expect(inspectImport([imported], [first]).conflicts).toHaveLength(1)
  })
  it('rejects reserved, fractional and out of range MCP ports', () => {
    for (const port of [0, 1023, 65536, 18766.5, NaN]) expect(validateSettings({ mcpPort: port, startOnLaunch: false, minimizeToTray: true })).toBeTruthy()
    expect(validateSettings({ mcpPort: 18766, startOnLaunch: false, minimizeToTray: true })).toBe('')
  })
})


describe('list and normalization behavior', () => {
  it('filters across address, group, source and authentication without mutating profiles', async () => {
    const { filterProfiles } = await import('../domain')
    const profiles = [
      { ...createProfile(), name: '服务 A', host: 'prod.local', username: 'ops', group: '生产', source: 'xshell' },
      { ...createProfile(), name: '服务 B', host: 'test.local', username: 'dev', group: '测试', source: 'tabby', authType: 'agent' as const },
    ]
    expect(filterProfiles(profiles, 'PROD', '生产', 'xshell', 'password')).toEqual([profiles[0]])
    expect(filterProfiles(profiles, 'prod', '测试', '', '')).toEqual([])
    expect(profiles).toHaveLength(2)
  })
  it('omits secrets for other auth modes without trimming password content', async () => {
    const { normalizeProfile } = await import('../domain')
    const profile = { ...createProfile(), name: ' A ', host: ' local ', username: ' ops ', password: ' secret ', passphrase: ' key ' }
    const normalized = normalizeProfile(profile)
    expect(normalized.name).toBe('A')
    expect(normalized.password).toBe(' secret ')
    expect(normalized.passphrase).toBeUndefined()
    expect(normalizeProfile({ ...profile, authType: 'agent' }).password).toBeUndefined()
  })
})


describe('deployment transfer roots', () => {
  it('rejects relative transfer directories and preserves explicit absolute roots', async () => {
    const { normalizeProfile } = await import('../domain')
    const profile = { ...createProfile(), name: '部署', host: 'prod.local', username: 'ops', tryKeyboard: true, allowedLocalPaths: ['build'], allowedRemotePaths: ['app'] }
    expect(validateProfile(profile, []).allowedLocalPaths).toBeTruthy()
    expect(validateProfile(profile, []).allowedRemotePaths).toBeTruthy()
    profile.allowedLocalPaths = [' F:/release ', 'F:/release', 'C:\\build']
    profile.allowedRemotePaths = ['/opt/apps']
    const normalized = normalizeProfile(profile)
    expect(validateProfile(normalized, [])).toEqual({})
    expect(normalized.allowedLocalPaths).toEqual(['F:/release', 'C:\\build'])
    expect(normalized.allowedRemotePaths).toEqual(['/opt/apps'])
  })
  it('requires a concrete SSH Agent endpoint', () => {
    const profile = { ...createProfile(), name: 'Agent', host: 'prod.local', username: 'ops', authType: 'agent' as const }
    expect(validateProfile(profile, []).agent).toBeTruthy()
    profile.agent = 'pageant'
    expect(validateProfile(profile, [])).toEqual({})
  })
})
