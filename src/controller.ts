import { reactive } from 'vue'
import type { DesktopBridge } from './api'
import { inspectImport, normalizeProfile, stripSecrets, validateProfile, validateSettings } from './domain'
import type { AppSettings, AuthChallenge, CodexRegistration, ConnectionProfile, ConnectionTest, EngineStatus, ImportPreview, SourceCandidate, SourceKind, RuntimeLogSnapshot, StorageInfo } from './types'

export function createAstraLink(bridge: DesktopBridge) {
  const state = reactive({
    desktop: bridge.available, loading: false, ready: false,
    profiles: [] as ConnectionProfile[], sources: [] as SourceCandidate[],
    settings: { mcpPort: 18766, startOnLaunch: false, minimizeToTray: true } as AppSettings,
    engine: { running: false, endpoint: '' } as EngineStatus,
    notice: null as null | { kind: 'success' | 'error' | 'info'; text: string },
    mutation: '', engineBusy: false, registrationBusy: false, discoveryBusy: false,
    previewBusy: false, preview: null as ImportPreview | null,
    results: {} as Record<string, ConnectionTest>, testing: {} as Record<string, boolean>,
    challenges: [] as AuthChallenge[], authBusy: '' as string,
    registration: null as CodexRegistration | null,
    clientRegistrations: {} as Record<string, CodexRegistration>,
    runtimeLogs: { entries: [], logPath: '', error: null } as RuntimeLogSnapshot,
    logsBusy: false, logsExportBusy: false,
    storageInfo: null as StorageInfo | null, storageInfoBusy: false,
  })
  let disposed = false
  let initialized = false
  let previewRevision = 0
  let engineRevision = 0
  let discoveryRevision = 0
  const revisions = new Map<string, number>()
  const completedChallenges = new Set<string>()
  const activeTests = new Map<string, symbol>()
  const challengeTests = new Map<string, symbol>()
  let engineKnown = false
  const unlisteners: (() => void)[] = []
  const nativeRequired = () => {
    if (!state.desktop) { state.notice = { kind: 'info', text: '请在AstraLink 桌面应用中执行此操作' }; return true }
    return disposed
  }
  function report(error: unknown, secrets: (string | undefined)[] = []) {
    let text = typeof error === 'string' ? error : error instanceof Error ? error.message : '操作失败，请重试'
    for (const secret of secrets) if (secret) text = text.split(secret).join('••••')
    state.notice = { kind: 'error', text }
  }
  function dismissChallenges(matches: (challenge: AuthChallenge) => boolean) {
    state.challenges = state.challenges.filter((challenge) => {
      if (!matches(challenge)) return true
      completedChallenges.add(challenge.requestId)
      challengeTests.delete(challenge.requestId)
      if (state.authBusy === challenge.requestId) state.authBusy = ''
      return false
    })
  }
  function receiveAuth(challenge: AuthChallenge) {
    if (disposed || (engineKnown && !state.engine.running) || completedChallenges.has(challenge.requestId) || state.challenges.some((item) => item.requestId === challenge.requestId)) return
    if (challenge.expiresAt <= Date.now()) return
    const test = activeTests.get(challenge.profileId)
    if (test) challengeTests.set(challenge.requestId, test)
    state.challenges.push(challenge)
  }
  function applyEngineStatus(engine: EngineStatus) {
    engineKnown = true
    state.engine = engine
    if (!engine.running) {
      dismissChallenges(() => true)
      for (const profileId of activeTests.keys()) invalidateResult(profileId)
    }
    // Completed successful tests are historical; they do not represent live connections.
  }
  function receiveEngine(engine: EngineStatus) {
    if (disposed) return
    engineRevision++
    applyEngineStatus(engine)
  }
  function invalidateResult(profileId: string) {
    revisions.set(profileId, (revisions.get(profileId) ?? 0) + 1)
    activeTests.delete(profileId)
    delete state.testing[profileId]
    delete state.results[profileId]
    dismissChallenges((challenge) => challenge.profileId === profileId)
  }
  function invalidateDependents(changedIds: Iterable<string>, profiles: ConnectionProfile[]) {
    const dependents = new Map<string, Set<string>>()
    for (const profile of profiles) {
      if (!profile.jumpHostId) continue
      const children = dependents.get(profile.jumpHostId) ?? new Set<string>()
      children.add(profile.id)
      dependents.set(profile.jumpHostId, children)
    }
    const visited = new Set<string>()
    const pending = [...changedIds]
    while (pending.length) {
      const id = pending.pop()!
      if (visited.has(id)) continue
      visited.add(id)
      invalidateResult(id)
      for (const dependent of dependents.get(id) ?? []) pending.push(dependent)
    }
  }
  function replaceProfiles(profiles: ConnectionProfile[], editedIds: string[] = []) {
    const old = new Map(state.profiles.map((profile) => [profile.id, JSON.stringify(stripSecrets(profile))]))
    const incoming = profiles.map(stripSecrets)
    const changedIds = new Set(editedIds)
    for (const id of new Set([...old.keys(), ...incoming.map((profile) => profile.id)])) {
      const updated = incoming.find((profile) => profile.id === id)
      if (!updated || (old.has(id) && old.get(id) !== JSON.stringify(updated))) changedIds.add(id)
    }
    // Keep old edges as well: deletion and reparenting can remove the affected dependency.
    invalidateDependents(changedIds, [...state.profiles, ...incoming])
    state.profiles = incoming
  }
  async function initialize() {
    if (nativeRequired() || initialized || state.loading) return
    state.loading = true
    try {
      if (!unlisteners.length) {
        const auth = await bridge.listen('astralink-auth', receiveAuth)
        if (disposed) { auth(); return }
        unlisteners.push(auth)
        try {
          const engine = await bridge.listen('astralink-engine', receiveEngine)
          if (disposed) { engine(); return }
          unlisteners.push(engine)
        } catch (error) {
          unlisteners.splice(0).forEach((stop) => stop())
          throw error
        }
      }
      const eventRevision = engineRevision
      const app = await bridge.invoke('get_app_state', {})
      if (disposed) return
      replaceProfiles(app.profiles)
      state.settings = app.settings
      if (eventRevision === engineRevision) applyEngineStatus(app.engine)
      initialized = true
      state.ready = true
      await discoverSources()
    } catch (error) { if (!disposed) report(error) }
    finally { state.loading = false }
  }
  async function discoverSources() {
    if (nativeRequired() || state.discoveryBusy) return
    const revision = ++discoveryRevision
    state.discoveryBusy = true
    try {
      const sources = await bridge.invoke('discover_sources', {})
      if (!disposed && revision === discoveryRevision) state.sources = sources
    } catch (error) { if (!disposed) report(error) }
    finally { if (revision === discoveryRevision) state.discoveryBusy = false }
  }
  function clearPreview() { previewRevision++; state.preview = null; state.previewBusy = false }
  async function previewImport(sourceKind: SourceKind, filePath: string, importPassword?: string) {
    if (nativeRequired() || state.mutation || !filePath.trim()) return false
    const revision = ++previewRevision
    state.preview = null
    state.previewBusy = true
    try {
      const preview = await bridge.invoke('preview_import', { sourceKind, filePath, ...(importPassword ? { importPassword } : {}) })
      if (disposed || revision !== previewRevision) return false
      state.preview = preview
      return true
    } catch (error) { if (revision === previewRevision && !disposed) report(error, [importPassword]); return false }
    finally { if (revision === previewRevision) state.previewBusy = false }
  }
  async function mutate(key: string, action: () => Promise<void>, secrets: (string | undefined)[] = []) {
    if (nativeRequired() || !state.ready || state.mutation) return false
    state.mutation = key
    state.notice = null
    try { await action(); return true }
    catch (error) { if (!disposed) report(error, secrets); return false }
    finally { state.mutation = '' }
  }
  async function saveProfile(profile: ConnectionProfile) {
    const normalized = normalizeProfile(profile)
    const errors = validateProfile(normalized, state.profiles)
    if (Object.keys(errors).length) { report(Object.values(errors)[0]); return false }
    return mutate('profile', async () => {
      const profiles = await bridge.invoke('upsert_profile', { profile: normalized })
      if (!disposed) { replaceProfiles(profiles, [profile.id]); state.notice = { kind: 'success', text: '服务器已保存' } }
    }, [profile.password, profile.passphrase, profile.totpSecret])
  }
  async function revealProfileSecret(profileId: string, field: 'password' | 'passphrase' | 'totpSecret') {
    if (nativeRequired() || !state.ready || !state.profiles.some((profile) => profile.id === profileId)) return null
    return bridge.invoke('reveal_profile_secret', { profileId, field })
  }
  async function deleteProfile(profileId: string) {
    return mutate('delete', async () => {
      const profiles = await bridge.invoke('delete_profile', { profileId })
      if (!disposed) { replaceProfiles(profiles, [profileId]); state.notice = { kind: 'success', text: '服务器已删除' } }
    })
  }
  async function deleteProfiles(ids: string[]) {
    const profileIds = [...new Set(ids)]
    if (!profileIds.length) return false
    return mutate('delete', async () => {
      const profiles = await bridge.invoke('delete_profiles', { profileIds })
      if (!disposed) { replaceProfiles(profiles, profileIds); state.notice = { kind: 'success', text: '已删除 ' + profileIds.length + ' 台服务器' } }
    })
  }
  async function applyImport(profiles: ConnectionProfile[], strategy: 'merge' | 'replace') {
    if (state.previewBusy || !state.preview || !profiles.length || state.preview.issues.some((issue) => issue.severity === 'error')) return false
    const inspection = inspectImport(profiles, strategy === 'replace' ? [] : state.profiles)
    if (inspection.errors.length) { report(inspection.errors[0]); return false }
    return mutate('import', async () => {
      const imported = await bridge.invoke('apply_import', { profiles, strategy })
      if (!disposed) {
        const editedIds = profiles.flatMap((profile) => [profile.id, ...state.profiles.filter((existing) => existing.name.trim().toLocaleLowerCase() === profile.name.trim().toLocaleLowerCase()).map((existing) => existing.id)])
        replaceProfiles(imported, editedIds)
        clearPreview()
        state.notice = { kind: 'success', text: `已导入 ${profiles.length} 台服务器` }
      }
    }, profiles.flatMap((profile) => [profile.password, profile.passphrase]))
  }
  async function saveSettings(settings: AppSettings) {
    const error = validateSettings(settings)
    if (error) { report(error); return false }
    return mutate('settings', async () => {
      const saved = await bridge.invoke('update_settings', { settings })
      if (!disposed) { state.settings = saved; state.notice = { kind: 'success', text: state.engine.running ? '设置已保存；端口变更将在重启 MCP 后生效' : '设置已保存' } }
    })
  }
  async function setEngine(running: boolean) {
    if (nativeRequired() || !state.ready || state.engineBusy || state.mutation) return false
    state.engineBusy = true
    const revision = engineRevision
    try {
      const engine = await bridge.invoke(running ? 'start_engine' : 'stop_engine', {})
      if (disposed) return false
      if (revision === engineRevision) applyEngineStatus(engine)
      if (state.engine.error) report(state.engine.error)
      else state.notice = { kind: state.engine.running === running ? 'success' : 'error', text: state.engine.running === running ? running ? 'MCP 服务已启动' : 'MCP 服务已停止' : '服务未达到请求的状态，请刷新检查' }
      return state.engine.running === running
    } catch (error) { if (!disposed) report(error); return false }
    finally { state.engineBusy = false }
  }
  async function refreshEngine() {
    if (nativeRequired() || !state.ready || state.engineBusy) return
    state.engineBusy = true
    const revision = engineRevision
    try {
      const engine = await bridge.invoke('get_engine_status', {})
      if (!disposed && revision === engineRevision) applyEngineStatus(engine)
    } catch (error) { if (!disposed) report(error) }
    finally { state.engineBusy = false }
  }
  async function registerCodex() {
    if (nativeRequired() || !state.ready || !state.engine.running || state.registrationBusy || state.engineBusy) return false
    state.registrationBusy = true
    state.registration = null
    try {
      const registration = await bridge.invoke('register_codex', {})
      if (!disposed) { state.registration = registration; state.notice = { kind: 'success', text: registration.reloadRequired ? '已接入 Codex，请重新加载 MCP 配置' : '已接入 Codex' } }
      return true
    } catch (error) { if (!disposed) report(error); return false }
    finally { state.registrationBusy = false }
  }
  async function registerClient(clientId: string, filePath?: string) {
    if (nativeRequired() || !state.ready || !state.engine.running || state.engineBusy || state.registrationBusy || state.mutation) return false
    state.registrationBusy = true
    delete state.clientRegistrations[clientId]
    try {
      const registration = await bridge.invoke('register_mcp_client', { clientId, ...(filePath ? { filePath } : {}) })
      if (!disposed) {
        state.clientRegistrations[clientId] = registration
        state.notice = { kind: 'success', text: registration.reloadRequired ? '客户端配置已更新，请重新加载 MCP' : '客户端配置已是最新' }
      }
      return true
    } catch (error) { if (!disposed) report(error); return false }
    finally { state.registrationBusy = false }
  }
  async function testConnection(profileId: string) {
    if (nativeRequired() || !state.ready || !state.engine.running || state.engineBusy || state.testing[profileId]) return
    const profile = state.profiles.find((item) => item.id === profileId)
    if (!profile || !profile.enabled) return
    const revision = revisions.get(profileId) ?? 0
    const test = Symbol(profileId)
    activeTests.set(profileId, test)
    const isCurrent = () => !disposed && activeTests.get(profileId) === test && revision === (revisions.get(profileId) ?? 0) && state.profiles.some((item) => item.id === profileId)
    const finishAuth = () => dismissChallenges((challenge) => challengeTests.get(challenge.requestId) === test)
    state.testing[profileId] = true
    delete state.results[profileId]
    try {
      const result = await bridge.invoke('test_connection', { profileId })
      if (isCurrent()) {
        state.results[profileId] = result
        // This command returns only after SSH succeeds or the attempt ends.
        finishAuth()
      }
    } catch (error) {
      if (isCurrent()) {
        state.results[profileId] = { status: 'failed', stage: 'request', detail: typeof error === 'string' ? error : error instanceof Error ? error.message : '连接测试失败', elapsedMs: 0 }
        finishAuth()
      }
    } finally {
      if (activeTests.get(profileId) === test) {
        activeTests.delete(profileId)
        delete state.testing[profileId]
      }
    }
  }
  async function submitOtp(requestId: string, code: string) {
    if (nativeRequired() || state.authBusy || !code.trim()) return false
    const challenge = state.challenges.find((item) => item.requestId === requestId)
    if (!challenge || challenge.expiresAt <= Date.now()) { report('认证请求已过期，请重新连接'); return false }
    if (challenge.prompts.length !== 1) { report('当前挑战包含多个提示，请取消后使用其他认证方式'); return false }
    state.authBusy = requestId
    state.notice = null
    try {
      const accepted = await bridge.invoke('submit_otp', { requestId, code })
      if (!state.challenges.some((challenge) => challenge.requestId === requestId)) return false
      if (!accepted) { report('认证请求已失效或未接受，请重试'); return false }
      dismissChallenges((challenge) => challenge.requestId === requestId)
      state.notice = { kind: 'info', text: '验证信息已提交，等待服务器认证结果' }
      return true
    } catch (error) {
      if (state.challenges.some((challenge) => challenge.requestId === requestId)) report(error, [code])
      return false
    } finally { if (state.authBusy === requestId) state.authBusy = '' }
  }
  async function cancelAuth(requestId: string) {
    if (nativeRequired() || state.authBusy) return false
    state.authBusy = requestId
    try {
      const accepted = await bridge.invoke('cancel_auth', { requestId })
      if (!state.challenges.some((challenge) => challenge.requestId === requestId)) return true
      // false means the backend request is already absent; it must not trap the modal.
      dismissChallenges((challenge) => challenge.requestId === requestId)
      state.notice = { kind: 'info', text: accepted ? '认证请求已取消' : '认证请求已结束' }
      return true
    } catch (error) {
      if (!state.challenges.some((challenge) => challenge.requestId === requestId)) return true
      report(error)
      return false
    } finally { if (state.authBusy === requestId) state.authBusy = '' }
  }
  function dispose() {
    disposed = true
    previewRevision++
    unlisteners.splice(0).forEach((stop) => stop())
    state.preview = null
    dismissChallenges(() => true)
    activeTests.clear()
  }
  async function refreshRuntimeLogs() {
    if (nativeRequired() || !state.ready || state.logsBusy) return
    state.logsBusy = true
    try {
      const logs = await bridge.invoke('get_runtime_logs', {})
      if (!disposed) state.runtimeLogs = logs
    } catch { if (!disposed) state.runtimeLogs.error = '读取运行日志失败，请重试' }
    finally { state.logsBusy = false }
  }
  async function refreshStorageInfo() {
    if (nativeRequired() || !state.ready || state.storageInfoBusy) return
    state.storageInfoBusy = true
    try { const info = await bridge.invoke('get_storage_info', {}); if (!disposed) state.storageInfo = info }
    catch { if (!disposed) report('读取存储信息失败，请重试') }
    finally { state.storageInfoBusy = false }
  }
  async function exportRuntimeLogs() {
    if (nativeRequired() || !state.ready || state.logsExportBusy) return false
    state.logsExportBusy = true
    try {
      if (!bridge.saveLogFile) throw new Error('当前环境无法打开日志保存窗口')
      const filePath = await bridge.saveLogFile()
      if (!filePath || disposed) return false
      await bridge.invoke('export_runtime_logs', { filePath })
      if (!disposed) state.notice = { kind: 'success', text: '运行日志已导出' }
      return true
    } catch (error) { if (!disposed) report(error); return false }
    finally { state.logsExportBusy = false }
  }
  async function refreshAppState() {
    if (nativeRequired() || !state.ready) return
    const app = await bridge.invoke('get_app_state', {})
    if (!disposed) {
      replaceProfiles(app.profiles)
      state.settings = app.settings
      applyEngineStatus(app.engine)
      await refreshStorageInfo()
    }
  }
  return { state, initialize, refreshAppState, discoverSources, clearPreview, previewImport, saveProfile, revealProfileSecret, deleteProfile, deleteProfiles, applyImport, saveSettings, setEngine, refreshEngine, registerCodex, registerClient, testConnection, submitOtp, cancelAuth, dispose, refreshRuntimeLogs, exportRuntimeLogs, refreshStorageInfo, pickFile: bridge.pickFile, pickClientConfig: () => bridge.pickFile(undefined, 'config'), report }
}
export type AstraLinkController = ReturnType<typeof createAstraLink>
