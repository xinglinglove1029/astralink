import type { RuntimeLogEntry } from './types'
export function filterRuntimeLogs(entries: RuntimeLogEntry[], level: string, profileId: string, query: string): RuntimeLogEntry[] {
  const needle = query.trim().toLocaleLowerCase()
  return entries.filter(entry => (!level || entry.level === level) && (!profileId || entry.profileId === profileId)
    && (!needle || [entry.message, entry.code, entry.source, entry.profileId ?? '', entry.requestId ?? ''].some(value => value.toLocaleLowerCase().includes(needle))))
    .slice().reverse()
}
export function formatRuntimeLogs(entries: RuntimeLogEntry[]): string {
  return entries.map(entry => `${new Date(entry.timestamp).toISOString()} [${entry.level.toUpperCase()}] ${entry.source} ${entry.code} ${entry.message} profile=${entry.profileId ?? '-'} request=${entry.requestId ?? '-'}`).join('\n')
}
