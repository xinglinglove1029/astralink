import { afterEach, expect, it, vi } from 'vitest'
import { currentWebUser, webBridge } from '../webBridge'

afterEach(() => {
  currentWebUser.value = null
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

it('does not poll authentication challenges for a read-only user', async () => {
  vi.useFakeTimers()
  const fetch = vi.fn().mockResolvedValue({ ok: true, status: 200, json: async () => [] })
  vi.stubGlobal('fetch', fetch)
  currentWebUser.value = { id: 'reader', username: 'reader', role: 'viewer', enabled: true, revision: 0, hasToken: false }
  const unsubscribe = await webBridge.listen('astralink-auth', () => {})
  try {
    await vi.advanceTimersByTimeAsync(4500)
    expect(fetch).not.toHaveBeenCalled()
    currentWebUser.value.role = 'operator'
    await vi.advanceTimersByTimeAsync(1500)
    expect(fetch).toHaveBeenCalledOnce()
    expect(fetch.mock.calls[0]?.[0]).toBe('/api/commands/get_auth_challenges')
  } finally {
    unsubscribe()
  }
})
