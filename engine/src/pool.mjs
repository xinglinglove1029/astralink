import { isDeepStrictEqual } from 'node:util';
import { randomUUID } from 'node:crypto';
import ssh2 from 'ssh2';
import { SSHConnectionManager, captureOperationSecrets } from './upstream.mjs';
import { EngineError } from './errors.mjs';
import { toSshConfig } from './profiles.mjs';
import { TotpScheduler } from './totp-scheduler.mjs';
function isJumpServerMenu(output) {
  const text = String(output ?? '').replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');
  return /\bJumpServer\b/i.test(text) && /(?:^|[\r\n])\s*Opt>\s*(?:$|[\r\n])/i.test(text);
}
const jumpServerMenuDetail = '认证成功，当前位于 JumpServer 资产菜单；请在高级配置填写目标资产和目标账号';
function isStoredPasswordPrompt(prompt) {
  const label = prompt.prompt.trim();
  // OTP and token labels take precedence; all ambiguous labels go to the UI.
  if (prompt.echo !== false || /\botp\b|one[\s-]*time|verification|authenticator|token|验证码|动态(?:密码|口令)|一次性(?:密码|口令)/i.test(label))
    return false;
  return /^(?:password|密码|口令)\s*[:：]?\s*$/i.test(label);
}
function isTotpPrompt(prompt) {
  // JumpServer echoes its explicit OTP prompt; echo controls display, not the answer type.
  if (/\botp\b|\bmfa\b|\b2fa\b|one[\s-]*time|验证码|动态(?:密码|口令)|一次性(?:密码|口令)/i.test(prompt.prompt))
    return true;
  return prompt.echo === false && /verification|authenticator|token/i.test(prompt.prompt);
}
class Connection extends SSHConnectionManager {
  constructor(profile, auth, jump, timeoutMs, pool) {
    super();
    this.profile = profile;
    this.auth = auth;
    this.jump = jump;
    this.timeoutMs = timeoutMs;
    this.pool = pool;
    this.attempts = new Set();
    this.configs = Object.assign(Object.create(null), { [profile.name]: toSshConfig(profile, timeoutMs) });
    this.defaultName = profile.name;
    this.sockets = new Set();
    this.stopped = false;
  }
  // No background status commands: control tests execute only hostname/id.
  scheduleStatusCollection() {
  }
  async connect(name = this.defaultName) {
    if (this.stopped)
      throw new EngineError('CONNECTION_CLOSED', '连接已关闭');
    if (this.hasUsableConnection(name)) {
      this.pool.diagnostics?.record('ssh_reused', this.profile.id, this.diagnosticRequestId);
      return;
    }
    if (this.pendingConnections.has(name))
      return this.pendingConnections.get(name);
    const pending = this.open(name);
    this.pendingConnections.set(name, pending);
    try {
      await pending;
    }
    finally {
      this.pendingConnections.delete(name);
    }
  }
  async open(name) {
    const client = new ssh2.Client();
    const config = this.configs[name], profile = this.profile;
    const traceId = this.diagnosticRequestId ?? randomUUID();
    const log = code => this.pool.diagnostics?.record(code, profile.id, traceId);
    log('ssh_connecting');
    const controller = new AbortController(), signal = controller.signal;
    const authOwner = Symbol(profile.id);
    const attempt = { controller, authPending: false, totpPending: false };
    this.attempts.add(attempt);
    this.sockets.add(client);
    let releaseJump, forwardedSocket, rejectReady, finished = false, cleaned = false, automaticAnswers = 0;
    const cleanup = () => {
      if (cleaned)
        return;
      cleaned = true;
      this.sockets.delete(client);
      this.auth.cancelOwner(authOwner);
      if (this.clients.get(name) === client)
        this.clearConnectionState(name);
      else if (!finished)
        this.cleanupShellState(name, true);
      forwardedSocket?.destroy();
      const release = releaseJump;
      releaseJump = undefined;
      release?.();
    };
    const abort = () => {
      cleanup();
      client.destroy();
      rejectReady?.(signal.reason);
    };
    signal.addEventListener('abort', abort, { once: true });
    // Start the single deadline before configuration, jump authentication or forwarding.
    const timer = setTimeout(() => controller.abort(new EngineError('SSH_CONNECTION_TIMEOUT', '连接超时')), this.timeoutMs);
    const wait = promise => new Promise((resolve, reject) => {
      const onAbort = () => {
        signal.removeEventListener('abort', onAbort);
        reject(signal.reason);
      };
      if (signal.aborted) {
        Promise.resolve(promise).catch(() => {
        });
        reject(signal.reason);
        return;
      }
      signal.addEventListener('abort', onAbort, { once: true });
      Promise.resolve(promise).then(value => {
        signal.removeEventListener('abort', onAbort);
        if (signal.aborted)
          reject(signal.reason);
        else
          resolve(value);
      }, error => {
        signal.removeEventListener('abort', onAbort);
        reject(error);
      });
    });
    const check = () => {
      if (signal.aborted)
        throw signal.reason;
      if (this.stopped)
        throw new EngineError('CONNECTION_CLOSED', '连接已关闭');
    };
    client.on('close', () => {
      cleanup();
      if (!finished && !signal.aborted)
        controller.abort(new EngineError('SSH_CONNECTION_CLOSED', '连接已关闭'));
    });
    client.on('error', error => {
      const reason = { ENOTFOUND: 'ssh_dns_failed', EAI_AGAIN: 'ssh_dns_failed', ECONNREFUSED: 'ssh_refused', ECONNRESET: 'ssh_reset' }[error.code];
      if (reason) log(reason);
      if (!finished)
        controller.abort(new EngineError(error.level === 'client-authentication' ? 'SSH_AUTH_FAILED' : 'SSH_CONNECTION_FAILED', error.level === 'client-authentication' ? '认证失败' : '连接失败'));
      else
        client.destroy();
    });
    try {
      const sshConfig = await wait(super.buildClientConfig(name, config));
      check();
      delete sshConfig.keyboard;
      if (this.jump) {
        const jump = this.jump;
        jump.active++;
        releaseJump = () => {
          jump.active--;
          jump.pool.disposeRetired(jump);
        };
        await wait(jump.manager.connect());
        check();
        forwardedSocket = await wait(new Promise((resolve, reject) => {
          jump.manager.getClient().forwardOut('127.0.0.1', 0, config.host, config.port, (error, socket) => {
            if (signal.aborted || this.stopped) {
              if (socket) {
                socket.once('error', () => {
                });
                socket.destroy();
              }
              reject(signal.reason ?? new EngineError('CONNECTION_CLOSED', '连接已关闭'));
              return;
            }
            if (error) {
              reject(new EngineError('SSH_CONNECTION_FAILED', '跳板转发失败'));
              return;
            }
            forwardedSocket = socket;
            resolve(socket);
          });
        }));
        check();
        sshConfig.sock = forwardedSocket;
      }
      await wait(new Promise((resolve, reject) => {
        rejectReady = reject;
        client.on('keyboard-interactive', (_title, instructions, _language, prompts, answer) => {
          if (signal.aborted || finished)
            return;
          const pendingIndexes = [];
          log(prompts.some(isTotpPrompt) ? (prompts.some(p => p.echo === true) ? 'auth_otp_echo' : 'auth_otp') : 'auth_unknown');
          const answers = prompts.map((prompt, index) => {
            if (profile.password && isStoredPasswordPrompt(prompt)) {
              log('auth_password');
              return profile.password;
            }
            pendingIndexes.push(index);
            return '';
          });
          if (!pendingIndexes.length) {
            answer(answers);
            return;
          }
          const otpPrompt = prompts.some(isTotpPrompt);
          if (profile.totpSecret && pendingIndexes.length === 1 && otpPrompt) {
            if (++automaticAnswers > 2) {
              controller.abort(new EngineError('AUTH_EXPIRED', '动态验证码被重复拒绝，请检查密钥和系统时间后重试'));
              return;
            }
            attempt.totpPending = true;
            void this.pool.totp.next(profile.totpSecret, { signal, onWait: log }).then(generated => {
              if (signal.aborted || finished) return;
              attempt.totpPending = false;
              answers[pendingIndexes[0]] = generated.code;
              log('otp_auto');
              answer(answers);
            }, () => {
              if (!signal.aborted && !finished)
                controller.abort(new EngineError('AUTH_EXPIRED', '动态验证码等待失败，请检查系统时间后重试'));
            }).catch(() => {
              if (!signal.aborted && !finished)
                controller.abort(new EngineError('SSH_AUTH_FAILED', '动态验证码提交失败'));
            });
            return;
          } else if (!otpPrompt) {
            log(!profile.totpSecret ? 'otp_missing' : 'otp_prompt_unknown');
          }
          log('auth_manual');
          attempt.authPending = true;
          void this.auth.challenge(profile, instructions, pendingIndexes.map(index => prompts[index]), authOwner).then(responses => {
            attempt.authPending = false;
            if (signal.aborted || finished)
              return;
            log('auth_manual_submitted');
            pendingIndexes.forEach((index, i) => {
              answers[index] = responses[i];
            });
            answer(answers);
          }, error => {
            log(error.code === 'AUTH_EXPIRED' ? 'auth_expired' : error.code === 'AUTH_CANCELLED' ? 'auth_cancelled' : 'auth_unsupported');
            attempt.authPending = false;
            controller.abort(error);
          });
        });
        client.once('ready', () => {
          log('ssh_authenticated');
          void (async () => {
            try {
              check();
              if (this.getTransportMode(config) === 'shell')
                await wait(this.initializeShellSession(client, name, config));
              check();
              this.clients.set(name, client);
              this.connected.set(name, true);
              resolve();
            }
            catch (error) {
              controller.abort(error);
            }
          })();
        });
        try {
          check();
          client.connect(sshConfig);
        }
        catch {
          controller.abort(new EngineError('SSH_CONNECTION_FAILED', '连接配置无效'));
        }
      }));
      finished = true;
    }
    catch (error) {
      if (!error.code?.startsWith('AUTH_')) log(error.code === 'SSH_AUTH_FAILED' ? 'ssh_auth_failed' : error.code === 'SSH_CONNECTION_TIMEOUT' ? 'ssh_timeout' : 'ssh_connection_failed');
      if (!signal.aborted)
        controller.abort(error);
      cleanup();
      client.destroy();
      throw signal.reason ?? error;
    }
    finally {
      clearTimeout(timer);
      signal.removeEventListener('abort', abort);
      this.attempts.delete(attempt);
    }
  }
  disconnect() {
    if (this.stopped)
      return;
    this.stopped = true;
    for (const attempt of this.attempts)
      attempt.controller.abort(new EngineError(attempt.authPending ? 'AUTH_CANCELLED' : 'CONNECTION_CLOSED', attempt.authPending ? '认证已取消' : '连接已关闭'));
    super.disconnect();
    for (const client of this.sockets)
      client.destroy();
    this.sockets.clear();
    this.configs = Object.create(null);
    this.profile = { id: this.profile.id };
  }
  cancelPendingAuthentication() {
    for (const attempt of this.attempts)
      if (attempt.totpPending)
        attempt.controller.abort(new EngineError('AUTH_CANCELLED', '连接配置已变更，认证已取消'));
  }
}
export class ConnectionPool {
  constructor(auth, { connectionTimeoutMs = 90000, now = Date.now, sleep, diagnostics } = {}) {
    this.auth = auth;
    this.diagnostics = diagnostics;
    this.timeoutMs = connectionTimeoutMs;
    this.now = now;
    this.totp = new TotpScheduler({ now, sleep });
    this.entries = new Map();
    this.retired = new Set();
    this.profiles = new Map();
    this.stopped = false;
  }
  update(profiles) {
    if (this.stopped)
      throw new EngineError('ENGINE_STOPPED', '服务已停止', 503);
    const byId = new Map(profiles.map(p => [p.id, p]));
    const enabled = profiles.filter(p => p.enabled);
    const fingerprint = p => {
      const chain = [p];
      let hop = p;
      while (hop.jumpHostId) {
        hop = byId.get(hop.jumpHostId.toLowerCase());
        chain.push(hop);
      }
      return chain;
    };
    const next = new Map();
    const make = p => {
      if (next.has(p.name))
        return next.get(p.name);
      const signature = fingerprint(p), existing = this.entries.get(p.name);
      if (existing && isDeepStrictEqual(existing.signature, signature)) {
        next.set(p.name, existing);
        return existing;
      }
      const jump = p.jumpHostId ? make(byId.get(p.jumpHostId.toLowerCase())) : undefined;
      const entry = { generation: Symbol(p.id), profile: p, signature, active: 0, retired: false, pool: this, manager: new Connection(p, this.auth, jump, this.timeoutMs, this) };
      next.set(p.name, entry);
      return entry;
    };
    for (const p of enabled)
      make(p);
    for (const [name, entry] of this.entries)
      if (next.get(name) !== entry) {
        entry.retired = true;
        entry.manager.cancelPendingAuthentication();
        this.retired.add(entry);
        this.disposeRetired(entry);
      }
    this.entries = next;
    this.profiles = byId;
    return profiles.length;
  }
  disposeRetired(entry) {
    if (entry.retired && entry.active === 0) {
      entry.manager.disconnect();
      entry.signature = [];
      entry.profile = { id: entry.profile.id };
      this.retired.delete(entry);
    }
  }
  secrets() {
    return [...this.entries.values(), ...this.retired].flatMap(entry => [entry.profile.password, entry.profile.passphrase, entry.profile.totpSecret]).filter(Boolean);
  }
  getAllServerInfos() {
    return [...this.entries.values()].flatMap(entry => entry.manager.getAllServerInfos());
  }
  async use(name, fn) {
    if (this.stopped)
      throw new EngineError('ENGINE_STOPPED', '服务已停止', 503);
    const entry = this.entries.get(name === undefined ? 'default' : name);
    if (!entry)
      throw new EngineError('PROFILE_NOT_FOUND', '连接不存在或未启用', 404);
    captureOperationSecrets(entry.generation, entry.signature);
    entry.active++;
    try {
      return await fn(entry.manager);
    }
    finally {
      entry.active--;
      this.disposeRetired(entry);
    }
  }
  executeCommand(command, directory, name, options) {
    return this.use(name, manager => manager.executeCommand(command, directory, undefined, options));
  }
  upload(local, remote, name) {
    return this.use(name, manager => manager.upload(local, remote));
  }
  download(remote, local, name) {
    return this.use(name, manager => manager.download(remote, local));
  }
  readFile(remote, options, name) {
    return this.use(name, manager => manager.readFile(remote, options));
  }
  writeFile(remote, content, options, name) {
    return this.use(name, manager => manager.writeFile(remote, content, options));
  }
  jobStart(command, directory, name) {
    return this.use(name, manager => manager.jobStart(command, directory));
  }
  jobStatus(id, options, name) {
    return this.use(name, manager => manager.jobStatus(id, options));
  }
  jobKill(id, name) {
    return this.use(name, manager => manager.jobKill(id));
  }
  jobList(limit, name) {
    return this.use(name, manager => manager.jobList(limit));
  }
  async test(profileId) {
    const start = Date.now(), profile = this.profiles.get(profileId);
    if (!profile || !profile.enabled)
      throw new EngineError('PROFILE_NOT_FOUND', '连接不存在或未启用', 404);
    let stage = 'connection';
    const traceId = randomUUID();
    const log = code => this.diagnostics?.record(code, profile.id, traceId);
    log('test_started');
    try {
      const stdout = await this.use(profile.name, async (manager) => {
        manager.diagnosticRequestId = traceId;
        await manager.connect();
        stage = 'command';
        log('test_command');
        return manager.executeCommand('hostname && id', undefined, undefined, { timeout: 10000 });
      });
      if (isJumpServerMenu(stdout)) {
        log('test_target_menu');
        return { status: 'partial', stage: 'target', detail: jumpServerMenuDetail, elapsedMs: Date.now() - start };
      }
      log('test_connected');
      return { status: 'connected', stage: 'command', detail: '连接成功', elapsedMs: Date.now() - start, stdout };
    }
    catch (error) {
      if (stage === 'command' && error.code === 'COMMAND_TIMEOUT' && isJumpServerMenu(error.message)) {
        log('test_target_menu');
        return { status: 'partial', stage: 'target', detail: jumpServerMenuDetail, elapsedMs: Date.now() - start };
      }
      if (stage === 'command') {
        log('test_failed');
        return { status: 'failed', stage, detail: error.code === 'COMMAND_TIMEOUT' ? '认证成功，但只读命令测试超时' : '认证成功，但只读命令执行失败', elapsedMs: Date.now() - start };
      }
      const auth = error.code?.startsWith('AUTH_') || ['SSH_AUTH_FAILED', 'SSH_AUTHENTICATION_MISSING'].includes(error.code);
      log(auth ? 'test_auth_incomplete' : 'test_failed');
      const authDetail = error.code === 'AUTH_EXPIRED' ? '认证已超时，请重新测试'
        : error.code === 'AUTH_CANCELLED' ? '认证已取消，请重新测试'
        : error.code === 'SSH_AUTH_FAILED' ? 'SSH 认证失败，请检查密码和动态验证码'
        : '认证未完成，请重新测试';
      return { status: auth ? 'auth_required' : 'failed', stage: auth ? 'authentication' : stage, detail: auth ? authDetail : '连接测试失败', elapsedMs: Date.now() - start };
    }
  }
  close() {
    this.stopped = true;
    this.totp.close();
    for (const entry of [...this.entries.values(), ...this.retired])
      entry.manager.disconnect();
    this.entries.clear();
    this.retired.clear();
    this.profiles.clear();
  }
}
