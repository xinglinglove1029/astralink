import { gt, valid } from 'semver'

export const RELEASE_PAGE = 'https://github.com/xinglinglove1029/astralink/releases/latest'
const RELEASE_API = 'https://api.github.com/repos/xinglinglove1029/astralink/releases/latest'

export type UpdateResult =
  | { status: 'available'; currentVersion: string; latestVersion: string }
  | { status: 'current'; currentVersion: string }
  | { status: 'unavailable'; currentVersion: string; reason: string }

function hasInstaller(assets: unknown, platform: string, version: string): boolean {
  if (!Array.isArray(assets)) return false
  const names = assets.flatMap((asset) => typeof asset?.name === 'string' ? [asset.name] : [])
    .filter((name) => name.startsWith(`Astralink_${version}_`))
  if (platform.includes('Win')) return names.some((name) => /_x64-setup\.exe$/i.test(name))
  if (platform.includes('Mac')) return names.some((name) => /_(?:aarch64|x64)\.dmg$/i.test(name))
  if (platform.includes('Linux')) return names.some((name) => /\.(?:AppImage|deb)$/i.test(name))
  return names.length > 0
}

export async function checkLatestRelease(
  currentVersion: string,
  platform: string,
  fetchRelease: typeof fetch = fetch,
): Promise<UpdateResult> {
  const current = valid(currentVersion)
  if (!current) return { status: 'unavailable', currentVersion, reason: '本机版本无效' }
  try {
    const response = await fetchRelease(RELEASE_API, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: AbortSignal.timeout(8000),
      cache: 'no-store',
    })
    if (!response.ok) throw new Error('release unavailable')
    const release: unknown = await response.json()
    if (!release || typeof release !== 'object') throw new Error('invalid release')
    const data = release as Record<string, unknown>
    const version = typeof data.tag_name === 'string' && /^v\d/.test(data.tag_name)
      ? valid(data.tag_name.slice(1)) : null
    if (!version || data.draft === true || data.prerelease === true || version.includes('-')) {
      throw new Error('invalid release')
    }
    if (!gt(version, current)) return { status: 'current', currentVersion }
    if (!hasInstaller(data.assets, platform, version)) {
      return { status: 'unavailable', currentVersion, reason: '新版尚无适用安装包' }
    }
    return { status: 'available', currentVersion, latestVersion: version }
  } catch {
    return { status: 'unavailable', currentVersion, reason: '无法连接版本服务，请稍后重试' }
  }
}
