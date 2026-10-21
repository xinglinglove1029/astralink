import { beforeEach, describe, expect, it, vi } from 'vitest'
const { openDialog, saveDialog } = vi.hoisted(() => ({ openDialog: vi.fn(), saveDialog: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: () => true }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openDialog, save: saveDialog }))
import { desktopBridge } from '../api'

describe('native Xshell selection', () => {
  it('uses a native save dialog with log extensions for diagnostic export', async () => {
    saveDialog.mockResolvedValue('C:/fixture/runtime.log')
    expect(await desktopBridge.saveLogFile?.()).toBe('C:/fixture/runtime.log')
    expect(saveDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '导出运行日志', defaultPath: 'astralink-runtime.log' }))
  })
  beforeEach(() => openDialog.mockReset())
  it('selects a directory for bulk session import without file filters', async () => {
    openDialog.mockResolvedValue('C:\\sessions')
    expect(await desktopBridge.pickFile('xshell', 'directory')).toBe('C:\\sessions')
    expect(openDialog).toHaveBeenCalledWith(expect.objectContaining({ directory: true, multiple: false }))
    expect(openDialog.mock.calls[0]![0].filters).toBeUndefined()
  })
  it('accepts Xshell session files and export archives through the file picker', async () => {
    openDialog.mockResolvedValue('C:\\sessions.xts')
    expect(await desktopBridge.pickFile('xshell')).toBe('C:\\sessions.xts')
    expect(openDialog).toHaveBeenCalledWith(expect.objectContaining({ directory: false, filters: expect.arrayContaining([expect.objectContaining({ extensions: ['xsh', 'xts'] })]) }))
  })
  it('uses a config picker for client configuration files', async () => {
    openDialog.mockResolvedValue('C:/fixture/config.toml')
    expect(await desktopBridge.pickFile(undefined, 'config')).toBe('C:/fixture/config.toml')
    expect(openDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '选择 AI 客户端配置文件', directory: false, filters: expect.arrayContaining([expect.objectContaining({ extensions: ['json', 'jsonc', 'json5', 'toml', 'yaml', 'yml'] })]) }))
  })
  it('keeps a cancelled native selection empty', async () => {
    openDialog.mockResolvedValue(null)
    expect(await desktopBridge.pickFile('xshell')).toBeNull()
  })
})
