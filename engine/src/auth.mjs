import { randomUUID } from 'node:crypto';
import { EngineError, redact } from './errors.mjs';
export class AuthQueue {
  constructor({ ttlMs = 60000, maxPending = 16, maxHistory = 128 } = {}) {
    for (const [key, value] of Object.entries({ ttlMs, maxPending, maxHistory }))
      if (!Number.isSafeInteger(value) || value < 1)
        throw new EngineError('INVALID_AUTH_LIMIT', key);
    if (maxHistory < maxPending)
      throw new EngineError('INVALID_AUTH_LIMIT', '事件历史必须覆盖待处理认证');
    this.ttlMs = ttlMs;
    this.maxPending = maxPending;
    this.maxHistory = maxHistory;
    this.pending = new Map();
    this.history = [];
    this.cursor = 0;
    this.stopped = false;
  }
  challenge(profile, instructions, prompts, owner = profile.id) {
    if (this.stopped)
      return Promise.reject(new EngineError('AUTH_CANCELLED', '认证已取消'));
    if (this.pending.size >= this.maxPending)
      return Promise.reject(new EngineError('AUTH_QUEUE_FULL', '认证请求过多'));
    if (!Array.isArray(prompts) || prompts.length !== 1)
      return Promise.reject(new EngineError('AUTH_PROMPTS_UNSUPPORTED', '当前认证需要多个回答，暂不支持'));
    const secrets = [profile.password, profile.passphrase, profile.totpSecret];
    const event = { requestId: randomUUID(), profileId: profile.id, connectionName: profile.name, host: profile.host, username: profile.username,
      instructions: redact(instructions, secrets).slice(0, 4096), prompts: prompts.map(p => ({ prompt: redact(p.prompt, secrets).slice(0, 1024), echo: p.echo === true })), expiresAt: Date.now() + this.ttlMs };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.settle(event.requestId, new EngineError('AUTH_EXPIRED', '认证已超时')), this.ttlMs);
      const cursor = ++this.cursor;
      this.pending.set(event.requestId, { event, timer, resolve, reject, owner, cursor });
      this.history.push({ cursor, event });
      if (this.history.length > this.maxHistory)
        this.history.shift();
    });
  }
  settle(id, error, answers) {
    const pending = this.pending.get(id);
    if (!pending)
      return false;
    this.pending.delete(id);
    clearTimeout(pending.timer);
    if (error)
      pending.reject(error);
    else
      pending.resolve(answers);
    return true;
  }
  respond({ requestId, code, cancel }) {
    const pending = this.pending.get(requestId);
    if (!pending)
      return false;
    if (Date.now() >= pending.event.expiresAt) {
      this.settle(requestId, new EngineError('AUTH_EXPIRED', '认证已超时'));
      return false;
    }
    if (cancel === true)
      return this.settle(requestId, new EngineError('AUTH_CANCELLED', '认证已取消'));
    if (typeof code !== 'string' || !code.length || code.length > 1024 || /[\x00-\x1f\x7f]/.test(code))
      throw new EngineError('INVALID_AUTH_RESPONSE', '认证回答无效');
    return this.settle(requestId, null, [code]);
  }
  events(since = 0) {
    return { cursor: this.cursor, events: [...this.pending.values()].filter(x => x.cursor > since).map(x => structuredClone(x.event)) };
  }
  cancelOwner(owner) {
    for (const [id, p] of this.pending)
      if (p.owner === owner)
        this.settle(id, new EngineError('AUTH_CANCELLED', '连接已关闭'));
  }
  cancelProfile(profileId) {
    for (const [id, p] of this.pending)
      if (p.event.profileId === profileId)
        this.settle(id, new EngineError('AUTH_CANCELLED', '连接已更新'));
  }
  close() {
    this.stopped = true;
    for (const id of this.pending.keys())
      this.settle(id, new EngineError('AUTH_CANCELLED', '认证已取消'));
    this.history = [];
  }
}
