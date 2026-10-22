import { describe, expect, it, vi } from 'vitest'
import { checkLatestRelease, RELEASE_PAGE } from '../updateCheck'

const release = (tag_name: string, assets: string[]) => ({
  tag_name, draft: false, prerelease: false, assets: assets.map((name) => ({ name })),
})
const respond = (body: unknown) => vi.fn(async (..._args: Parameters<typeof fetch>) => ({ ok: true, json: async () => body }) as Response)

describe('installed version checks', () => {
  it('reports a newer stable Windows installer and only uses the fixed release page', async () => {
    const fetcher = respond(release('v0.2.0', ['Astralink_0.2.0_x64-setup.exe']))
    expect(await checkLatestRelease('0.1.0', 'Windows', fetcher)).toEqual({
      status: 'available', currentVersion: '0.1.0', latestVersion: '0.2.0',
    })
    expect(fetcher.mock.calls[0]?.[0]).toContain('api.github.com/repos/xinglinglove1029/astralink/releases/latest')
    expect(RELEASE_PAGE).toBe('https://github.com/xinglinglove1029/astralink/releases/latest')
  })

  it('does not claim an update without a matching installer', async () => {
    expect((await checkLatestRelease('0.1.0', 'Windows', respond(release('v0.2.0', ['Astralink_0.2.0_amd64.deb'])))).status).toBe('unavailable')
    expect((await checkLatestRelease('0.1.0', 'Windows', respond(release('v0.2.0', ['Astralink_0.1.0_x64-setup.exe'])))).status).toBe('unavailable')
    expect((await checkLatestRelease('0.2.0', 'Windows', respond(release('v0.2.0', [])))).status).toBe('current')
  })

  it('rejects prereleases, malformed versions, and network errors without claiming currency', async () => {
    expect((await checkLatestRelease('0.1.0', 'Mac', respond(release('v0.2.0-beta.1', ['Astralink_0.2.0_aarch64.dmg'])))).status).toBe('unavailable')
    expect((await checkLatestRelease('bad', 'Windows', respond(release('v0.2.0', [])))).status).toBe('unavailable')
    expect((await checkLatestRelease('0.1.0', 'Linux', vi.fn(async () => { throw new Error('offline') }))).status).toBe('unavailable')
    expect((await checkLatestRelease('0.1.0', 'Linux', respond(release('v0.2.0', ['Astralink_0.2.0_amd64.AppImage'])))).status).toBe('available')
  })
})
