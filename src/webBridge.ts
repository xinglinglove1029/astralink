import type { DesktopBridge } from './api'
import type { CommandMap, EventMap, SourceKind } from './types'

let csrf = ''
let backupFile: File | null = null
const isWeb = () => typeof document !== 'undefined' && !!document.querySelector('meta[name="astralink-web"]')
export const webMode = isWeb()

async function request(path: string, body?: unknown, raw = false): Promise<unknown> {
  const response = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin',
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json', 'X-AstraLink-CSRF': csrf },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (response.status === 401 && !path.includes('/auth/')) window.dispatchEvent(new Event('astralink-session-expired'))
  if (!response.ok) {
    const error = await response.json().catch(() => ({})) as { error?: string }
    throw new Error(error.error ?? '请求失败')
  }
  return raw ? response.blob() : response.json()
}
export async function checkWebAuth(): Promise<boolean> {
  const result = await request('/api/auth/status') as { authenticated: boolean; csrf?: string }
  csrf = result.csrf ?? ''
  return result.authenticated
}
export async function loginWeb(password: string): Promise<void> {
  const result = await request('/api/auth/login', { password }) as { csrf: string }
  csrf = result.csrf
}
export async function logoutWeb(): Promise<void> { await request('/api/auth/logout', {}); csrf = '' }
export async function changeWebPassword(oldPassword: string, newPassword: string): Promise<void> {
  await request('/api/auth/password', { oldPassword, newPassword }); csrf = ''
}

async function chooseFile(): Promise<File | null> {
  return new Promise(resolve => {
    const input = document.createElement('input')
    input.type = 'file'
    input.onchange = () => resolve(input.files?.[0] ?? null)
    input.oncancel = () => resolve(null)
    input.click()
  })
}
async function upload(source: SourceKind | 'key', file: File): Promise<string> {
  const ext = (file.name.split('.').pop() ?? 'txt').replace(/[^a-zA-Z0-9]/g, '').slice(0, 16) || 'txt'
  const response = await fetch(`/api/upload/${source}/${ext}`, {
    method: 'POST', credentials: 'same-origin', headers: { 'X-AstraLink-CSRF': csrf, 'Content-Type': 'application/octet-stream' }, body: file,
  })
  if (!response.ok) throw new Error((await response.json().catch(() => ({})) as { error?: string }).error ?? '上传失败')
  const result = await response.json() as { id?: string; path?: string }
  return result.path ?? result.id ?? ''
}
async function encodedBackup(): Promise<string> {
  if (!backupFile) throw new Error('请重新选择备份文件')
  if (backupFile.size > 32 * 1024 * 1024) throw new Error('备份文件超过大小限制')
  const bytes = new Uint8Array(await backupFile.arrayBuffer())
  let binary = ''
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192))
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}
function download(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a'); anchor.href = url; anchor.download = filename; anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 30000)
}

export const webBridge: DesktopBridge = {
  available: true,
  platform: 'web',
  async invoke<K extends keyof CommandMap>(command: K, args: CommandMap[K]['args']): Promise<CommandMap[K]['result']> {
    if (command === 'export_runtime_logs') {
      const logs = await request('/api/commands/get_runtime_logs', {}) as CommandMap['get_runtime_logs']['result']
      download(new Blob([logs.entries.map(entry => JSON.stringify(entry)).join('\n') + '\n'], { type: 'application/x-ndjson' }), 'astralink-runtime.jsonl')
      return undefined as CommandMap[K]['result']
    }
    if (command === 'export_backup_file') {
      const params = args as CommandMap['export_backup_file']['args']
      download(await request('/api/backup/export', { passphrase: params.passphrase }, true) as Blob, 'astralink.backup')
      return undefined as CommandMap[K]['result']
    }
    if (command === 'preview_backup_file' || command === 'restore_backup_file') {
      const params = args as CommandMap['restore_backup_file']['args']
      return request(`/api/backup/${command === 'preview_backup_file' ? 'preview' : 'import'}`, { ...params, data: await encodedBackup() }) as Promise<CommandMap[K]['result']>
    }
    return request(`/api/commands/${command}`, args) as Promise<CommandMap[K]['result']>
  },
  async listen<K extends keyof EventMap>(event: K, handler: (payload: EventMap[K]) => void): Promise<() => void> {
    let active = true
    const poll = async () => {
      if (!active) return
      try {
        const command = event === 'astralink-auth' ? 'get_auth_challenges' : 'get_engine_status'
        const result = await request(`/api/commands/${command}`, {})
        if (!active) return
        if (event === 'astralink-auth') {
          for (const challenge of result as EventMap['astralink-auth'][]) handler(challenge as EventMap[K])
        } else handler(result as EventMap[K])
      } catch { /* The next poll retries; API errors are shown by direct commands. */ }
    }
    const timer = setInterval(poll, 1500)
    return () => { active = false; clearInterval(timer) }
  },
  async pickFile(source?: SourceKind, selection?: 'file' | 'directory' | 'config') {
    if (selection === 'directory' || selection === 'config') return null
    const file = await chooseFile()
    return file ? upload(source ?? 'key', file) : null
  },
  async pickBackupFile() { backupFile = await chooseFile(); return backupFile?.name ?? null },
  async saveBackupFile() { return 'astralink.backup' },
  async saveLogFile() { return 'astralink-runtime.jsonl' },
}
