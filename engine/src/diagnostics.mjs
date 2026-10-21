// No free-form fields: untrusted prompts, errors, commands and secrets never enter this channel.
const CODES = new Set([
  'engine_ready', 'profiles_synced', 'ssh_connecting', 'ssh_reused', 'ssh_authenticated', 'ssh_auth_failed',
  'ssh_connection_failed', 'ssh_timeout', 'ssh_dns_failed', 'ssh_refused', 'ssh_reset', 'auth_otp', 'auth_otp_echo', 'auth_password', 'auth_unknown',
  'otp_auto', 'otp_near_expiry', 'otp_step_used', 'otp_missing', 'otp_prompt_unknown',
  'auth_manual', 'auth_manual_submitted', 'auth_cancelled', 'auth_expired', 'auth_unsupported',
  'test_started', 'test_command', 'test_connected', 'test_target_menu', 'test_failed', 'test_auth_incomplete',
  'tool_started', 'tool_finished', 'tool_failed',
]);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
export class Diagnostics {
  constructor(limit = 512) { this.limit = limit; this.cursor = 0; this.entries = []; }
  record(code, profileId, requestId) {
    if (!CODES.has(code) || (profileId !== undefined && !UUID.test(profileId)) || (requestId !== undefined && !UUID.test(requestId))) return;
    const entry = { seq: ++this.cursor, timestamp: Date.now(), code,
      ...(profileId ? { profileId } : {}), ...(requestId ? { requestId } : {}) };
    this.entries.push(entry);
    if (this.entries.length > this.limit) this.entries.shift();
  }
  since(since = 0) {
    return { cursor: this.cursor, dropped: since < (this.entries[0]?.seq ?? 1) - 1,
      entries: this.entries.filter(entry => entry.seq > since).map(entry => ({ ...entry })) };
  }
}
