import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { CommandMap, EventMap, SourceKind } from './types'
import { webBridge, webMode } from './webBridge'

export interface DesktopBridge {
  available: boolean
  platform?: 'desktop' | 'web'
  invoke<K extends keyof CommandMap>(command: K, args: CommandMap[K]['args']): Promise<CommandMap[K]['result']>
  listen<K extends keyof EventMap>(event: K, handler: (payload: EventMap[K]) => void): Promise<() => void>
  pickFile(source?: SourceKind, selection?: 'file' | 'directory' | 'config'): Promise<string | null>
  saveLogFile?(): Promise<string | null>
  pickBackupFile?(): Promise<string | null>
  saveBackupFile?(): Promise<string | null>
}
const filters: Record<SourceKind, { name: string; extensions: string[] }> = {
  xshell: { name: 'Xshell 会话或导出包', extensions: ['xsh', 'xts'] },
  finalshell: { name: 'FinalShell 连接配置', extensions: ['json'] },
  nyaterm: { name: 'NyaTerm 数据库或 v1 配置', extensions: ['redb', 'json'] },
  mobaxterm: { name: 'MobaXterm 会话书签', extensions: ['mxtsessions'] },
  securecrt: { name: 'SecureCRT 导出设置', extensions: ['xml'] },
  electerm: { name: 'Electerm 书签', extensions: ['json'] },
  windterm: { name: 'WindTerm 会话', extensions: ['sessions'] },
  termius: { name: 'Termius 数据目录', extensions: ['*'] },
  'nya-backup': { name: 'NyaTerm 加密备份', extensions: ['nya'] },
  tabby: { name: 'Tabby 配置', extensions: ['yaml', 'yml'] },
  netcatty: { name: 'Netcatty 导出', extensions: ['csv'] },
  'mcp-json': { name: 'MCP 配置', extensions: ['json'] },
  openssh: { name: 'OpenSSH 配置', extensions: ['*'] },
}
export const desktopBridge: DesktopBridge = {
  available: isTauri(),
  platform: 'desktop',
  invoke: (command, args) => invoke(command, args),
  listen: (event, handler) => listen<EventMap[typeof event]>(event, ({ payload }) => handler(payload)),
  saveLogFile: () => save({ title: '导出运行日志', defaultPath: 'astralink-runtime.log', filters: [{ name: '运行日志', extensions: ['log', 'txt'] }] }),
  pickBackupFile: () => open({ multiple: false, directory: false, title: '选择 AstraLink 备份', filters: [{ name: 'AstraLink 加密备份', extensions: ['backup'] }] }).then(file => typeof file === 'string' ? file : null),
  saveBackupFile: () => save({ title: '导出 AstraLink 备份', defaultPath: 'astralink.backup', filters: [{ name: 'AstraLink 加密备份', extensions: ['backup'] }] }),
  pickFile: async (source, selection = 'file') => {
    const file = await open({ multiple: false, directory: selection === 'directory', title: selection === 'config' ? '选择 AI 客户端配置文件' : selection === 'directory' ? '选择连接配置目录' : source ? '选择导入文件' : '选择私钥文件', filters: selection === 'config' ? [{ name: '客户端配置', extensions: ['json', 'jsonc', 'json5', 'toml', 'yaml', 'yml'] }, { name: '所有文件', extensions: ['*'] }] : selection === 'directory' ? undefined : source ? [filters[source], { name: '所有文件', extensions: ['*'] }] : undefined })
    return typeof file === 'string' ? file : null
  },
}
export const activeBridge = webMode ? webBridge : desktopBridge
