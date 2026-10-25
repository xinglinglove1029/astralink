import { appendFileSync, existsSync, lstatSync, renameSync, statSync } from 'node:fs';

export function recordToolCall(file, entry) {
  if (!file) return;
  if (existsSync(file) && lstatSync(file).isSymbolicLink()) throw new Error('audit path invalid');
  if (existsSync(file) && statSync(file).size > 8 * 1024 * 1024)
    renameSync(file, `${file}.1`);
  const safe = {
    timestamp: Date.now(), actorId: entry.actorId, actor: entry.actor,
    channel: 'mcp', action: entry.action, outcome: entry.outcome,
    profileId: entry.profileId ?? null, durationMs: Math.round(entry.durationMs),
  };
  appendFileSync(file, `${JSON.stringify(safe)}\n`, { encoding: 'utf8', mode: 0o600, flag: 'a' });
}
