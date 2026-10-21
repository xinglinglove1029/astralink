export type AuthType = 'password' | 'privateKey' | 'agent'
export type SecretField = 'password' | 'passphrase' | 'totpSecret'
export type SourceKind = 'xshell' | 'tabby' | 'netcatty' | 'mcp-json' | 'openssh'
export interface ConnectionProfile {
  id: string
  name: string
  host: string
  port: number
  username: string
  group: string
  source: string
  authType: AuthType
  privateKeyPath?: string
  password?: string
  passphrase?: string
  totpSecret?: string
  hasPassword: boolean
  hasPassphrase: boolean
  hasTotpSecret: boolean
  agent?: string
  transportMode: 'exec' | 'shell'
  tryKeyboard: boolean
  enabled: boolean
  notes: string
  jumpHostId?: string
  targetHost?: string
  targetUsername?: string
  allowedLocalPaths?: string[]
  allowedRemotePaths?: string[]
}
export interface AppSettings { mcpPort: number; startOnLaunch: boolean; minimizeToTray: boolean }
export interface EngineStatus { running: boolean; endpoint: string; pid?: number; version?: string; error?: string }
export interface AppState { profiles: ConnectionProfile[]; settings: AppSettings; engine: EngineStatus }
export interface ImportIssue { severity: 'warning' | 'error'; message: string; profileName?: string }
export interface ImportPreview {
  source: string
  sourcePath: string
  profiles: ConnectionProfile[]
  issues: ImportIssue[]
  stats: { total: number; ready: number; needsCredential: number; unsupported: number }
}
export interface SourceCandidate { kind: string; label: string; path: string; exists: boolean; profileCount?: number }
export interface AuthChallenge {
  requestId: string
  profileId: string
  connectionName: string
  host: string
  username: string
  instructions: string
  prompts: { prompt: string; echo: boolean }[]
  expiresAt: number
}
export interface ConnectionTest { status: 'connected' | 'partial' | 'auth_required' | 'failed'; stage: string; detail: string; elapsedMs: number; stdout?: string }
export interface CodexRegistration { configPath: string; backupPath?: string; reloadRequired: boolean }
export interface RuntimeLogEntry { id: number; timestamp: number; level: 'info' | 'warn' | 'error'; source: string; code: string; message: string; profileId?: string; requestId?: string }
export interface RuntimeLogSnapshot { entries: RuntimeLogEntry[]; logPath: string; error: string | null }
export interface StorageInfo { backend: 'sqlite'; databasePath: string; legacyBackupPath: string | null; serverCount: number }
export interface CommandMap {
  get_app_state: { args: Record<string, never>; result: AppState }
  get_storage_info: { args: Record<string, never>; result: StorageInfo }
  get_runtime_logs: { args: Record<string, never>; result: RuntimeLogSnapshot }
  export_runtime_logs: { args: { filePath: string }; result: void }
  discover_sources: { args: Record<string, never>; result: SourceCandidate[] }
  preview_import: { args: { sourceKind: SourceKind; filePath: string }; result: ImportPreview }
  apply_import: { args: { profiles: ConnectionProfile[]; strategy: 'merge' | 'replace' }; result: ConnectionProfile[] }
  upsert_profile: { args: { profile: ConnectionProfile }; result: ConnectionProfile[] }
  reveal_profile_secret: { args: { profileId: string; field: SecretField }; result: string | null }
  delete_profile: { args: { profileId: string }; result: ConnectionProfile[] }
  delete_profiles: { args: { profileIds: string[] }; result: ConnectionProfile[] }
  update_settings: { args: { settings: AppSettings }; result: AppSettings }
  start_engine: { args: Record<string, never>; result: EngineStatus }
  stop_engine: { args: Record<string, never>; result: EngineStatus }
  get_engine_status: { args: Record<string, never>; result: EngineStatus }
  test_connection: { args: { profileId: string }; result: ConnectionTest }
  submit_otp: { args: { requestId: string; code: string }; result: boolean }
  cancel_auth: { args: { requestId: string }; result: boolean }
  register_codex: { args: Record<string, never>; result: CodexRegistration }
  register_mcp_client: { args: { clientId: string; filePath?: string }; result: CodexRegistration }
  get_mcp_token: { args: Record<string, never>; result: string }
}
export interface EventMap { 'astralink-auth': AuthChallenge; 'astralink-engine': EngineStatus }
