import { createHash } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { normalizeTotpSecret, totp } from './totp.mjs';

/** Reserve OTP steps by secret fingerprint, including across different asset profiles. */
export class TotpScheduler {
  constructor({ now = Date.now, sleep = (ms, signal) => delay(ms, undefined, { signal }) } = {}) {
    this.now = now;
    this.sleep = sleep;
    this.used = new Map();
    this.latestStep = -1;
    this.lifetime = new AbortController();
  }
  async next(secret, { signal, onWait = () => {} } = {}) {
    const combined = signal ? AbortSignal.any([signal, this.lifetime.signal]) : this.lifetime.signal;
    const key = createHash('sha256').update(normalizeTotpSecret(secret)).digest('hex');
    for (;;) {
      combined.throwIfAborted();
      const generated = totp(secret, this.now());
      if (generated.step < this.latestStep)
        throw new Error('系统时间回退，请校准时间后重试');
      if (generated.step > this.latestStep) {
        for (const [fingerprint, step] of this.used)
          if (step < generated.step - 1) this.used.delete(fingerprint);
        this.latestStep = generated.step;
      }
      const previous = this.used.get(key);
      if (generated.remainingMs >= 5000 && (previous === undefined || generated.step > previous)) {
        if (!this.used.has(key) && this.used.size >= 1024) throw new Error('动态验证码请求过多，请稍后重试');
        this.used.set(key, generated.step);
        return generated;
      }
      // A rollback must not make an already-submitted timestep reusable.
      if (previous !== undefined && generated.step < previous)
        throw new Error('系统时间回退，请校准时间后重试');
      onWait(generated.remainingMs < 5000 ? 'otp_near_expiry' : 'otp_step_used');
      await this.sleep(generated.remainingMs + 50, combined);
    }
  }
  close() { this.lifetime.abort(); this.used.clear(); }
}
