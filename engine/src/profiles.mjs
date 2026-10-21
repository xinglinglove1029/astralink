import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import ssh2 from 'ssh2';
import { normalizeRemotePath } from '@fangjunjie/ssh-mcp-server/build/utils/remote-path.js';
import { EngineError } from './errors.mjs';
import { normalizeTotpSecret } from './totp.mjs';
export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const invalid = field => {
  throw new EngineError('INVALID_PROFILE', '连接配置无效：' + field);
};
const text = (value, field, max = 1024, optional = false) => {
  if (optional && value === undefined)
    return undefined;
  if (typeof value !== 'string' || (!optional && !value.trim()) || value.length > max || /[\x00-\x1f\x7f]/.test(value))
    invalid(field);
  return value;
};
const notesText = value => {
  if (value === undefined)
    return '';
  if (typeof value !== 'string' || value.length > 4096 || /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]|\r(?!\n)/.test(value))
    invalid('notes');
  // Preserve imported LF/CRLF endings and tabs exactly, without normalization.
  return value;
};
// Explicit roots supplement upstream's cwd restriction; no disk-wide defaults.
const pathAllowlist = (value, field) => {
  if (!Array.isArray(value) || value.length > 32)
    invalid(field);
  const paths = value.map(value => {
    const item = text(value, field, 4096);
    if (item !== item.trim() || /[*?]/.test(item))
      invalid(field);
    if (field === 'allowedLocalPaths') {
      const windowsAbsolute = /^[A-Za-z]:[\\/]/.test(item) || /^\\\\[^\\/]+[\\/][^\\/]+(?:[\\/]|$)/.test(item);
      if (!path.isAbsolute(item) || (process.platform === 'win32' && !windowsAbsolute))
        invalid(field);
      if (process.platform === 'win32' && /[<>|"]/.test(item))
        invalid(field);
      return path.normalize(item);
    }
    if (item.startsWith('//') || item.startsWith('\\'))
      invalid(field);
    try {
      return normalizeRemotePath(item);
    }
    catch {
      invalid(field);
    }
  });
  return Object.freeze(paths);
};
export async function validateProfiles(input) {
  if (!Array.isArray(input) || input.length > 512)
    invalid('profiles');
  const ids = new Set(), names = new Set();
  const profiles = [];
  for (const raw of input) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw))
      invalid('profile');
    if (typeof raw.id !== 'string' || !UUID.test(raw.id) || ids.has(raw.id.toLowerCase()))
      invalid('id');
    const name = text(raw.name, 'name', 128);
    if (name !== name.trim() || names.has(name))
      invalid('name');
    const host = text(raw.host, 'host', 253);
    if (/[\s/@?#]/.test(host))
      invalid('host');
    if (!Number.isInteger(raw.port) || raw.port < 1 || raw.port > 65535)
      invalid('port');
    if (!['password', 'privateKey', 'agent'].includes(raw.authType))
      invalid('authType');
    if (!['exec', 'shell'].includes(raw.transportMode))
      invalid('transportMode');
    for (const field of ['enabled', 'tryKeyboard', 'hasPassword', 'hasPassphrase'])
      if (typeof raw[field] !== 'boolean')
        invalid(field);
    if (raw.hasTotpSecret !== undefined && typeof raw.hasTotpSecret !== 'boolean')
      invalid('hasTotpSecret');
    for (const field of ['password', 'passphrase'])
      if (raw[field] !== undefined && (typeof raw[field] !== 'string' || raw[field].length > 16384 || raw[field].includes('\0')))
        invalid(field);
    const p = {
      id: raw.id.toLowerCase(), name, host, port: raw.port, username: text(raw.username, 'username', 256),
      authType: raw.authType, transportMode: raw.transportMode, enabled: raw.enabled, tryKeyboard: raw.tryKeyboard,
      hasPassword: raw.hasPassword, hasPassphrase: raw.hasPassphrase,
      hasTotpSecret: raw.hasTotpSecret === true,
      group: text(raw.group ?? '', 'group', 256, true), source: text(raw.source ?? '', 'source', 128, true), notes: notesText(raw.notes),
    };
    for (const field of ['privateKeyPath', 'agent', 'jumpHostId', 'targetHost', 'targetUsername'])
      if (raw[field] !== undefined && raw[field] !== '')
        p[field] = text(raw[field], field, 4096);
    for (const field of ['allowedLocalPaths', 'allowedRemotePaths'])
      if (raw[field] !== undefined)
        p[field] = pathAllowlist(raw[field], field);
    for (const field of ['password', 'passphrase'])
      if (raw[field] !== undefined)
        p[field] = raw[field];
    if (raw.totpSecret !== undefined) {
      try { p.totpSecret = normalizeTotpSecret(raw.totpSecret); }
      catch { invalid('totpSecret'); }
    }
    if (p.hasTotpSecret !== Boolean(p.totpSecret))
      invalid('hasTotpSecret');
    if (p.authType === 'agent' && !p.agent)
      invalid('agent');
    if (p.authType === 'privateKey') {
      if (!p.privateKeyPath || !path.isAbsolute(p.privateKeyPath))
        invalid('privateKeyPath');
      try {
        const metadata = await stat(p.privateKeyPath);
        if (!metadata.isFile() || metadata.size > 1024 * 1024)
          invalid('privateKeyPath');
        const parsed = ssh2.utils.parseKey(await readFile(p.privateKeyPath), p.passphrase);
        if (parsed instanceof Error || Array.isArray(parsed) || !parsed.isPrivateKey())
          invalid('privateKeyPath');
      }
      catch {
        invalid('privateKeyPath');
      }
    }
    if (Boolean(p.targetHost) !== Boolean(p.targetUsername))
      invalid('targetHost/targetUsername');
    if (p.targetHost)
      for (const field of ['username', 'targetHost', 'targetUsername'])
        if (/[\s@#]/.test(p[field]))
          invalid(field);
    ids.add(p.id);
    names.add(name);
    profiles.push(Object.freeze(p));
  }
  const byId = new Map(profiles.map(p => [p.id, p]));
  for (const p of profiles) {
    const visited = new Set([p.id]);
    let current = p;
    while (current.jumpHostId) {
      if (!UUID.test(current.jumpHostId))
        invalid('jumpHostId');
      current = byId.get(current.jumpHostId.toLowerCase());
      if (!current || (p.enabled && !current.enabled))
        throw new EngineError('UNRESOLVED_JUMP_HOST', '跳板连接不存在或未启用');
      if (visited.has(current.id) || visited.size >= 8)
        throw new EngineError('INVALID_JUMP_CHAIN', '跳板连接循环或层级过多');
      visited.add(current.id);
    }
  }
  return profiles;
}
export function toSshConfig(p, connectionTimeoutMs = 90000) {
  return {
    host: p.host, port: p.port, username: p.targetHost ? p.username + '@' + p.targetUsername + '@' + p.targetHost : p.username,
    transportMode: p.transportMode, tryKeyboard: p.tryKeyboard,
    ...(p.allowedLocalPaths !== undefined ? { allowedLocalPaths: p.allowedLocalPaths } : {}),
    ...(p.allowedRemotePaths !== undefined ? { allowedRemotePaths: p.allowedRemotePaths } : {}),
    ...(p.authType === 'password' && p.password ? { password: p.password } : {}),
    ...(p.authType === 'privateKey' ? { privateKey: p.privateKeyPath, ...(p.passphrase ? { passphrase: p.passphrase } : {}) } : {}),
    ...(p.authType === 'agent' ? { agent: p.agent } : {}), connectionTimeoutMs,
  };
}
