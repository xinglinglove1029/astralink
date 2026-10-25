import { afterEach, expect, it, vi } from 'vitest'
import { copyText } from '../clipboard'

afterEach(() => vi.unstubAllGlobals())
function setup(clipboard?: { writeText: ReturnType<typeof vi.fn> }, copied = true) {
  const field = { value: '', readOnly: false, style: { cssText: '' }, setAttribute: vi.fn(), focus: vi.fn(), select: vi.fn(), setSelectionRange: vi.fn(), remove: vi.fn() }
  const active = { focus: vi.fn() }
  const append = vi.fn()
  const exec = vi.fn(() => copied)
  vi.stubGlobal('navigator', { clipboard })
  vi.stubGlobal('window', { getSelection: () => null })
  vi.stubGlobal('document', { activeElement: active, createElement: () => field, querySelectorAll: () => [], body: { append }, execCommand: exec })
  return { field, active, append, exec }
}
it('copies on plain HTTP without the Clipboard API and clears the temporary secret', async () => {
  const { field, active, exec } = setup()
  await copyText('synthetic-token')
  expect(exec).toHaveBeenCalledWith('copy')
  expect(field.remove).toHaveBeenCalledOnce()
  expect(field.value).toBe('')
  expect(active.focus).toHaveBeenCalledOnce()
})
it('falls back when clipboard permission is denied', async () => {
  const { exec } = setup({ writeText: vi.fn().mockRejectedValue(new Error('denied')) })
  await copyText('configuration')
  expect(exec).toHaveBeenCalledOnce()
})
it('reports failure and removes temporary content when both copy methods fail', async () => {
  const { field } = setup(undefined, false)
  await expect(copyText('synthetic-token')).rejects.toThrow('剪贴板不可用')
  expect(field.value).toBe('')
  expect(field.remove).toHaveBeenCalledOnce()
})
it('prefers modern clipboard when available', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  const { exec } = setup({ writeText })
  await copyText('configuration')
  expect(writeText).toHaveBeenCalledWith('configuration')
  expect(exec).not.toHaveBeenCalled()
})
