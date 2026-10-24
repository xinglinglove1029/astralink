import { describe, expect, it } from 'vitest'
import { createAstraLink } from '../controller'
import { createProfile } from '../domain'
import type { DesktopBridge } from '../api'
import type { AppState, AuthChallenge, ConnectionProfile, ConnectionTest, EngineStatus } from '../types'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}
function setup(overrides: Record<string, (args?: any) => any> = {}) {
  const profile = { ...createProfile(), name: '服务器', host: 'localhost', username: 'test', hasPassword: true }
  const app: AppState = { profiles: [profile], settings: { mcpPort: 18766, startOnLaunch: false, minimizeToTray: true }, engine: { running: true, endpoint: 'http://127.0.0.1:18766/mcp' } }
  const handlers = new Map<string, (payload: any) => void>()
  const calls: string[] = []
  const bridge: DesktopBridge = {
    available: true,
    invoke: async (name, args) => { calls.push(name); return overrides[name] ? overrides[name](args) : name === 'get_app_state' ? app : name === 'discover_sources' ? [] : true },
    listen: async (name, callback) => { handlers.set(name, callback); return () => handlers.delete(name) },
    pickFile: async () => null,
  }
  return { dock: createAstraLink(bridge), app, profile, handlers, calls }
}

describe('async desktop state', () => {
  it('keeps the server-provided MCP client IP through initial load and refresh', async () => {
    const { dock, app } = setup()
    app.mcpClientHost = '192.168.97.240'
    await dock.initialize()
    expect(dock.state.mcpClientHost).toBe('192.168.97.240')
    app.mcpClientHost = undefined
    await dock.refreshAppState()
    expect(dock.state.mcpClientHost).toBe('127.0.0.1')
  })
  it('loads explicit SQLite storage locations without returning credentials', async () => {
    const info={backend:'sqlite',databasePath:'C:/fixture/astralink.db',legacyBackupPath:'C:/fixture/state.json',serverCount:21}
    const { dock,calls }=setup({get_storage_info:()=>info})
    await dock.initialize()
    expect(calls).not.toContain('get_storage_info')
    await dock.refreshStorageInfo()
    expect(dock.state.storageInfo).toEqual(info)
    expect(dock.state.storageInfoBusy).toBe(false)
  })
  it('loads diagnostic snapshots on explicit demand without overlapping requests', async () => {
    const pending = deferred<any>()
    const { dock, calls } = setup({ get_runtime_logs: () => pending.promise })
    await dock.initialize()
    expect(calls).not.toContain('get_runtime_logs')
    const first = dock.refreshRuntimeLogs()
    await dock.refreshRuntimeLogs()
    expect(calls.filter(c => c === 'get_runtime_logs')).toHaveLength(1)
    pending.resolve({ entries: [], logPath: 'C:/fixture/logs/runtime.jsonl', error: '写盘失败' })
    await first
    expect(dock.state.runtimeLogs.error).toBe('写盘失败')
    expect(dock.state.logsBusy).toBe(false)
  })
  it('cancels a native log export without writing a file or reporting success', async () => {
    const calls: string[] = []
    const bridge: DesktopBridge = { available:true,invoke:async command=>{ calls.push(command); return undefined as any },listen:async()=>()=>{},pickFile:async()=>null,saveLogFile:async()=>null }
    const dock = createAstraLink(bridge)
    dock.state.ready = true
    expect(await dock.exportRuntimeLogs()).toBe(false)
    expect(calls).toEqual([])
    expect(dock.state.logsExportBusy).toBe(false)
    expect(dock.state.notice).toBeNull()
  })
  it('reads a saved password only when explicitly requested without adding it to profile state', async () => {
    const { dock, profile, calls } = setup({ reveal_profile_secret: ({ profileId, field }) => {
      expect(profileId).toBe(profile.id)
      expect(field).toBe('password')
      return 'stored-test-password'
    } })
    await dock.initialize()
    expect(calls).not.toContain('reveal_profile_secret')
    expect(await dock.revealProfileSecret(profile.id, 'password')).toBe('stored-test-password')
    expect(dock.state.profiles[0]?.password).toBeUndefined()
    expect(await dock.revealProfileSecret('missing', 'password')).toBeNull()
    expect(calls.filter((call) => call === 'reveal_profile_secret')).toHaveLength(1)
  })
  it('never invokes native actions from browser preview', async () => {
    let calls = 0
    const dock = createAstraLink({ available: false, invoke: async () => { calls++; throw Error() }, listen: async () => () => {}, pickFile: async () => null })
    await dock.initialize()
    await dock.setEngine(true)
    await dock.registerCodex()
    expect(calls).toBe(0)
    expect(dock.state.desktop).toBe(false)
    expect(dock.state.engine.running).toBe(false)
  })
  it('discards an old import preview after another file is chosen', async () => {
    const old = deferred<any>()
    const fresh = deferred<any>()
    const { dock } = setup({ preview_import: (args) => args.filePath === 'old' ? old.promise : fresh.promise })
    const a = dock.previewImport('xshell', 'old')
    const b = dock.previewImport('tabby', 'new')
    fresh.resolve({ source: 'tabby', sourcePath: 'new', profiles: [], issues: [], stats: { total: 0, ready: 0, needsCredential: 0, unsupported: 0 } })
    await b
    old.resolve({ source: 'xshell', sourcePath: 'old', profiles: [], issues: [], stats: { total: 0, ready: 0, needsCredential: 0, unsupported: 0 } })
    await a
    expect(dock.state.preview?.sourcePath).toBe('new')
    expect(dock.state.previewBusy).toBe(false)
  })
  it('does not overwrite an engine event with a stale initial snapshot', async () => {
    const initial = deferred<AppState>()
    const { dock, app, handlers } = setup({ get_app_state: () => initial.promise })
    const task = dock.initialize()
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
    const event: EngineStatus = { running: false, endpoint: '', error: '进程退出' }
    handlers.get('astralink-engine')!(event)
    initial.resolve(app)
    await task
    expect(dock.state.engine).toEqual(event)
  })
  it('serializes profile mutations so late responses cannot resurrect deleted hosts', async () => {
    const saved = deferred<any>()
    const { dock, profile, calls } = setup({ upsert_profile: () => saved.promise })
    await dock.initialize()
    const task = dock.saveProfile(profile)
    expect(await dock.deleteProfile(profile.id)).toBe(false)
    expect(calls).not.toContain('delete_profile')
    saved.resolve([profile]); await task
  })
  it('discards a connection result if its profile changes during the test', async () => {
    const result = deferred<any>()
    const { dock, profile } = setup({ test_connection: () => result.promise, upsert_profile: (args) => [args.profile] })
    await dock.initialize()
    const task = dock.testConnection(profile.id)
    await dock.saveProfile({ ...profile, host: 'changed.local' })
    result.resolve({ status: 'connected', stage: 'command', detail: 'old host', elapsedMs: 10 })
    await task
    expect(dock.state.results[profile.id]).toBeUndefined()
  })
  it('deduplicates OTP events and rejects expired submissions before IPC', async () => {
    const { dock, handlers, calls } = setup()
    await dock.initialize()
    const challenge: AuthChallenge = { requestId: 'request', profileId: 'profile', connectionName: '服务器', host: 'host', username: 'ops', instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 }
    handlers.get('astralink-auth')!(challenge)
    handlers.get('astralink-auth')!(challenge)
    expect(dock.state.challenges).toHaveLength(1)
    dock.state.challenges[0]!.expiresAt = Date.now() - 1
    expect(await dock.submitOtp('request', '123456')).toBe(false)
    expect(calls).not.toContain('submit_otp')
  })
  it('does not treat a refused OTP as success', async () => {
    const { dock, handlers } = setup({ submit_otp: () => false })
    await dock.initialize()
    handlers.get('astralink-auth')!({ requestId: 'request', profileId: 'profile', connectionName: 'server', host: 'host', username: 'ops', instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    expect(await dock.submitOtp('request', '123456')).toBe(false)
    expect(dock.state.challenges).toHaveLength(1)
    expect(dock.state.notice?.kind).toBe('error')
  })
})

describe('auth and credential boundaries', () => {
  it('receives and resolves OTP while a connection test is still pending', async () => {
    const pending = deferred<any>()
    const { dock, profile, handlers, calls } = setup({ test_connection: () => pending.promise })
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    handlers.get('astralink-auth')!({ requestId: 'otp', profileId: profile.id, connectionName: profile.name, host: profile.host, username: profile.username, instructions: '验证码', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    expect(dock.state.testing[profile.id]).toBe(true)
    expect(dock.state.challenges).toHaveLength(1)
    expect(await dock.submitOtp('otp', '123456')).toBe(true)
    expect(dock.state.testing[profile.id]).toBe(true)
    expect(calls).toContain('submit_otp')
    expect(dock.state.notice?.text).toContain('等待')
    pending.resolve({ status: 'connected', stage: 'command', detail: '已执行 hostname/id', elapsedMs: 30, stdout: 'actual-host' })
    await test
    expect(dock.state.results[profile.id]?.stdout).toBe('actual-host')
    expect(dock.state.testing[profile.id]).toBeUndefined()
  })
  it('never stores returned password material in profile metadata', async () => {
    const { dock, profile } = setup({ upsert_profile: (args) => [{ ...args.profile, hasPassword: true }] })
    await dock.initialize()
    expect(await dock.saveProfile({ ...profile, password: 'secret', passphrase: 'key-secret' })).toBe(true)
    expect(dock.state.profiles[0]?.password).toBeUndefined()
    expect(dock.state.profiles[0]?.passphrase).toBeUndefined()
    expect(dock.state.profiles[0]?.hasPassword).toBe(true)
  })
  it('redacts submitted secrets in operation errors', async () => {
    const { dock, profile } = setup({ upsert_profile: () => { throw new Error('rejected secret-value') } })
    await dock.initialize()
    await dock.saveProfile({ ...profile, password: 'secret-value' })
    expect(dock.state.notice?.text).not.toContain('secret-value')
    expect(dock.state.notice?.kind).toBe('error')
  })
  it('removes event listeners when disposed during initialization', async () => {
    const snapshot = deferred<AppState>()
    const { dock, app, handlers } = setup({ get_app_state: () => snapshot.promise })
    const initialization = dock.initialize()
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
    dock.dispose()
    snapshot.resolve(app)
    await initialization
    expect(handlers.size).toBe(0)
    expect(dock.state.ready).toBe(false)
  })
  it('blocks multi-prompt responses instead of inventing a code format', async () => {
    const { dock, handlers, calls } = setup()
    await dock.initialize()
    handlers.get('astralink-auth')!({ requestId: 'multi', profileId: 'profile', connectionName: 'server', host: 'host', username: 'ops', instructions: '', prompts: [{ prompt: 'OTP', echo: false }, { prompt: 'Password', echo: false }], expiresAt: Date.now() + 60000 })
    expect(await dock.submitOtp('multi', '123456')).toBe(false)
    expect(calls).not.toContain('submit_otp')
    expect(dock.state.challenges).toHaveLength(1)
  })
  it('does not issue duplicate start requests and preserves an event received during startup', async () => {
    const start = deferred<EngineStatus>()
    const { dock, handlers, calls } = setup({ start_engine: () => start.promise })
    await dock.initialize()
    const first = dock.setEngine(true)
    expect(await dock.setEngine(true)).toBe(false)
    const failure = { running: false, endpoint: '', error: '引擎异常退出' }
    handlers.get('astralink-engine')!(failure)
    start.resolve({ running: true, endpoint: 'stale' })
    expect(await first).toBe(false)
    expect(dock.state.engine).toEqual(failure)
    expect(calls.filter((command) => command === 'start_engine')).toHaveLength(1)
    expect(dock.state.notice?.kind).toBe('error')
  })
})

describe('profile credential changes', () => {
  it('invalidates a test when only the saved password changes', async () => {
    const result = deferred<any>()
    const { dock, profile } = setup({ test_connection: () => result.promise, upsert_profile: (args) => [{ ...args.profile, password: undefined, hasPassword: true }] })
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    await dock.saveProfile({ ...profile, password: 'different-password' })
    result.resolve({ status: 'connected', stage: 'command', detail: 'old credential', elapsedMs: 50 })
    await test
    expect(dock.state.results[profile.id]).toBeUndefined()
  })
})

describe('auth request lifecycle', () => {
  function challenge(profileId: string, requestId: string): AuthChallenge {
    return { requestId, profileId, connectionName: '服务器', host: 'host', username: 'ops', instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 }
  }
  it('dismisses an unexpired request that the backend no longer has', async () => {
    const { dock, profile, handlers } = setup({ cancel_auth: () => false })
    await dock.initialize()
    const otp = challenge(profile.id, 'already-ended')
    handlers.get('astralink-auth')!(otp)
    expect(await dock.cancelAuth(otp.requestId)).toBe(true)
    expect(dock.state.challenges).toEqual([])
    expect(dock.state.notice?.kind).toBe('info')
    handlers.get('astralink-auth')!(otp)
    expect(dock.state.challenges).toEqual([])
  })
  it('clears every challenge when the engine stops and ignores late duplicates', async () => {
    const { dock, profile, handlers } = setup()
    await dock.initialize()
    const first = challenge(profile.id, 'engine-first')
    const second = challenge('external-profile', 'engine-second')
    handlers.get('astralink-auth')!(first)
    handlers.get('astralink-auth')!(second)
    handlers.get('astralink-engine')!({ running: false, endpoint: '', error: '断开连接' })
    expect(dock.state.challenges).toEqual([])
    handlers.get('astralink-auth')!(first)
    expect(dock.state.challenges).toEqual([])
  })
  it('clears a completed connection challenge without closing another pending profile', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, profile, handlers } = setup({ test_connection: () => pending.promise })
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    handlers.get('astralink-auth')!(challenge(profile.id, 'finished-test'))
    handlers.get('astralink-auth')!(challenge('other-profile', 'still-pending'))
    pending.resolve({ status: 'failed', stage: 'authentication', detail: '服务器关闭连接', elapsedMs: 10 })
    await test
    expect(dock.state.challenges.map((item) => item.requestId)).toEqual(['still-pending'])
    expect(dock.state.testing[profile.id]).toBeUndefined()
  })
  it('clears a challenge when test_connection rejects', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, profile, handlers } = setup({ test_connection: () => pending.promise.then(() => { throw Error('断开连接') }) })
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    handlers.get('astralink-auth')!(challenge(profile.id, 'rejected-test'))
    pending.resolve({ status: 'failed', stage: 'network', detail: '', elapsedMs: 10 })
    await test
    expect(dock.state.challenges).toEqual([])
    expect(dock.state.results[profile.id]?.status).toBe('failed')
  })
  it('clears an ended authentication challenge after the blocking test returns', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, profile, handlers } = setup({ test_connection: () => pending.promise })
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    handlers.get('astralink-auth')!(challenge(profile.id, 'waiting-auth'))
    pending.resolve({ status: 'auth_required', stage: 'authentication', detail: '认证已超时，请重新测试', elapsedMs: 10 })
    await test
    expect(dock.state.challenges).toHaveLength(0)
    expect(dock.state.testing[profile.id]).toBeUndefined()
    expect(await dock.submitOtp('waiting-auth', '123456')).toBe(false)
    handlers.get('astralink-auth')!(challenge(profile.id, 'waiting-auth'))
    expect(dock.state.challenges).toHaveLength(0)
  })
})

function dependencySetup(overrides: Record<string, (args?: any) => any> = {}) {
  let profiles: ConnectionProfile[] = []
  const env = setup({
    upsert_profile: (args) => {
      profiles = profiles.map((profile) => profile.id === args.profile.id ? { ...args.profile, password: undefined, passphrase: undefined } : profile)
      return profiles
    },
    delete_profile: (args) => { profiles = profiles.filter((profile) => profile.id !== args.profileId); return profiles },
    ...overrides,
  })
  const jump = { ...env.profile, id: 'jump', name: '跳板机' }
  const middle = { ...env.profile, id: 'middle', name: '中间跳板', jumpHostId: jump.id }
  const target = { ...env.profile, id: 'target', name: '目标服务器', jumpHostId: middle.id }
  const unrelated = { ...env.profile, id: 'unrelated', name: '独立服务器' }
  profiles = [jump, middle, target, unrelated]
  env.app.profiles = profiles
  return { ...env, jump, middle, target, unrelated }
}
const connectedResult: ConnectionTest = { status: 'connected', stage: 'command', detail: '旧配置测试成功', elapsedMs: 10 }

describe('transitive jump-host test invalidation', () => {
  it('invalidates pending and completed dependents after editing a jump host address', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, jump, middle, target, unrelated } = dependencySetup({ test_connection: (args) => args.profileId === target.id ? pending.promise : connectedResult })
    await dock.initialize()
    await dock.testConnection(middle.id)
    await dock.testConnection(unrelated.id)
    const oldTest = dock.testConnection(target.id)
    expect(await dock.saveProfile({ ...jump, host: 'new-jump.local' })).toBe(true)
    expect(dock.state.testing[target.id]).toBeUndefined()
    expect(dock.state.results[middle.id]).toBeUndefined()
    expect(dock.state.results[unrelated.id]).toEqual(connectedResult)
    pending.resolve(connectedResult)
    await oldTest
    expect(dock.state.results[target.id]).toBeUndefined()
  })
  it('invalidates transitive dependents when only jump credentials change', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, jump, middle, target } = dependencySetup({ test_connection: (args) => args.profileId === target.id ? pending.promise : connectedResult })
    await dock.initialize()
    await dock.testConnection(middle.id)
    const oldTest = dock.testConnection(target.id)
    expect(await dock.saveProfile({ ...jump, password: 'new-jump-password' })).toBe(true)
    expect(dock.state.results[middle.id]).toBeUndefined()
    pending.resolve(connectedResult)
    await oldTest
    expect(dock.state.results[target.id]).toBeUndefined()
  })
  it('uses the previous dependency graph when a jump profile is removed', async () => {
    const pending = deferred<ConnectionTest>()
    const { dock, jump, middle, target, unrelated } = dependencySetup({ test_connection: (args) => args.profileId === target.id ? pending.promise : connectedResult })
    await dock.initialize()
    await dock.testConnection(middle.id)
    await dock.testConnection(unrelated.id)
    const oldTest = dock.testConnection(target.id)
    expect(await dock.deleteProfile(jump.id)).toBe(true)
    expect(dock.state.results[middle.id]).toBeUndefined()
    expect(dock.state.testing[target.id]).toBeUndefined()
    expect(dock.state.results[unrelated.id]).toEqual(connectedResult)
    pending.resolve(connectedResult)
    await oldTest
    expect(dock.state.results[target.id]).toBeUndefined()
  })
  it('does not let an invalidated old test clear a new pending test or its OTP', async () => {
    const oldPending = deferred<ConnectionTest>()
    const newPending = deferred<ConnectionTest>()
    let count = 0
    const { dock, jump, target, handlers } = dependencySetup({ test_connection: () => ++count === 1 ? oldPending.promise : newPending.promise })
    await dock.initialize()
    const oldTest = dock.testConnection(target.id)
    await dock.saveProfile({ ...jump, host: 'new-jump.local' })
    const newTest = dock.testConnection(target.id)
    handlers.get('astralink-auth')!({ requestId: 'new-test-otp', profileId: target.id, connectionName: target.name, host: target.host, username: target.username, instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    oldPending.resolve(connectedResult)
    await oldTest
    expect(count).toBe(2)
    expect(dock.state.testing[target.id]).toBe(true)
    expect(dock.state.challenges.map((item) => item.requestId)).toEqual(['new-test-otp'])
    expect(dock.state.results[target.id]).toBeUndefined()
    newPending.resolve({ ...connectedResult, detail: '新配置测试成功' })
    await newTest
    expect(dock.state.results[target.id]?.detail).toBe('新配置测试成功')
    expect(dock.state.testing[target.id]).toBeUndefined()
    expect(dock.state.challenges).toEqual([])
  })
})


describe('pending auth isolation', () => {
  it('keeps a genuine auth event received before the initial profile snapshot', async () => {
    const initial = deferred<AppState>()
    const { dock, app, profile, handlers } = setup({ get_app_state: () => initial.promise })
    const initialize = dock.initialize()
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
    handlers.get('astralink-auth')!({ requestId: 'startup-auth', profileId: profile.id, connectionName: profile.name, host: profile.host, username: profile.username, instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    initial.resolve(app)
    await initialize
    expect(dock.state.challenges.map((item) => item.requestId)).toEqual(['startup-auth'])
  })
  it('keeps a preexisting MCP challenge when an independent test of the same profile finishes', async () => {
    const { dock, profile, handlers } = setup({ test_connection: () => connectedResult })
    await dock.initialize()
    handlers.get('astralink-auth')!({ requestId: 'external-auth', profileId: profile.id, connectionName: profile.name, host: profile.host, username: profile.username, instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    await dock.testConnection(profile.id)
    expect(dock.state.challenges.map((item) => item.requestId)).toEqual(['external-auth'])
  })
  it('does not let a late OTP response after engine stop clear the next busy auth request', async () => {
    const oldSubmission = deferred<boolean>()
    const nextSubmission = deferred<boolean>()
    const { dock, profile, handlers } = setup({ submit_otp: (args) => args.requestId === 'old-submit' ? oldSubmission.promise : nextSubmission.promise })
    await dock.initialize()
    const emitChallenge = (requestId: string) => handlers.get('astralink-auth')!({ requestId, profileId: profile.id, connectionName: profile.name, host: profile.host, username: profile.username, instructions: '', prompts: [{ prompt: 'OTP', echo: false }], expiresAt: Date.now() + 60000 })
    emitChallenge('old-submit')
    const old = dock.submitOtp('old-submit', '123456')
    handlers.get('astralink-engine')!({ running: false, endpoint: '' })
    expect(dock.state.challenges).toEqual([])
    expect(dock.state.authBusy).toBe('')
    handlers.get('astralink-engine')!({ running: true, endpoint: 'http://127.0.0.1:18766/mcp' })
    emitChallenge('new-submit')
    const fresh = dock.submitOtp('new-submit', '654321')
    oldSubmission.resolve(true)
    expect(await old).toBe(false)
    expect(dock.state.authBusy).toBe('new-submit')
    expect(dock.state.challenges.map((item) => item.requestId)).toEqual(['new-submit'])
    nextSubmission.resolve(true)
    expect(await fresh).toBe(true)
  })
})


describe('atomic batch deletion', () => {
  it('uses one batch command, clears deleted results and ignores late tests', async () => {
    const pending = deferred<ConnectionTest>()
    let ids: string[] = []
    const { dock, profile, app, calls } = setup({ test_connection: () => pending.promise, delete_profiles: (args) => { ids = args.profileIds; return [] } })
    const second = { ...profile, id: 'second', name: '另一台' }
    app.profiles.push(second)
    await dock.initialize()
    const test = dock.testConnection(profile.id)
    expect(await dock.deleteProfiles([profile.id, second.id, profile.id])).toBe(true)
    expect(ids).toEqual([profile.id, second.id])
    expect(calls.filter((name) => name === 'delete_profiles')).toHaveLength(1)
    expect(calls).not.toContain('delete_profile')
    pending.resolve({ status: 'connected', stage: 'command', detail: 'late', elapsedMs: 10 })
    await test
    expect(dock.state.profiles).toEqual([])
    expect(dock.state.results[profile.id]).toBeUndefined()
    expect(dock.state.testing[profile.id]).toBeUndefined()
  })
  it('keeps the list and results when a batch fails and rejects empty selection', async () => {
    const { dock, profile, calls } = setup({ delete_profiles: () => { throw Error('连接仍被用作跳板') } })
    await dock.initialize()
    dock.state.results[profile.id] = { status: 'connected', stage: 'command', detail: 'saved', elapsedMs: 1 }
    expect(await dock.deleteProfiles([])).toBe(false)
    expect(calls).not.toContain('delete_profiles')
    expect(await dock.deleteProfiles([profile.id])).toBe(false)
    expect(dock.state.profiles).toHaveLength(1)
    expect(dock.state.results[profile.id]?.detail).toBe('saved')
    expect(dock.state.notice?.kind).toBe('error')
  })
})

describe('client-specific quick setup', () => {
  it('passes client and optional selected file and keeps results separate', async () => {
    const registrations: any[] = []
    const { dock } = setup({ register_mcp_client: (args) => { registrations.push(args); return { configPath: args.filePath || 'default-config', reloadRequired: true } } })
    await dock.initialize()
    expect(await dock.registerClient('cursor')).toBe(true)
    expect(await dock.registerClient('hermes', 'C:/fixture/config.yaml')).toBe(true)
    expect(registrations).toEqual([{ clientId: 'cursor' }, { clientId: 'hermes', filePath: 'C:/fixture/config.yaml' }])
    expect(dock.state.clientRegistrations.cursor?.configPath).toBe('default-config')
    expect(dock.state.clientRegistrations.hermes?.configPath).toBe('C:/fixture/config.yaml')
  })
  it('rejects configuration while service is stopped and serializes setup actions', async () => {
    const pending = deferred<any>()
    const { dock, calls } = setup({ register_mcp_client: () => pending.promise })
    await dock.initialize()
    dock.state.engine.running = false
    expect(await dock.registerClient('cursor')).toBe(false)
    expect(calls).not.toContain('register_mcp_client')
    dock.state.engine.running = true
    const task = dock.registerClient('cursor')
    expect(await dock.registerClient('hermes')).toBe(false)
    pending.resolve({ configPath: 'fixture', reloadRequired: true })
    expect(await task).toBe(true)
    expect(calls.filter((name) => name === 'register_mcp_client')).toHaveLength(1)
  })
})
