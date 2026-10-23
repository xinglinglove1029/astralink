import assert from 'node:assert/strict';
import { generateKeyPairSync } from 'node:crypto';
import ssh2 from 'ssh2';
import http from 'node:http';
import net from 'node:net';
import path from 'node:path';
import { spawn } from 'node:child_process';
export const tokens = { controlToken: 'control-test-secret', mcpToken: 'mcp-test-secret' };
export const profile = (patch = {}) => ({ id: '11111111-1111-4111-8111-111111111111', name: 'test', host: '127.0.0.1', port: 22, username: 'tester', group: '', source: 'manual', authType: 'password', hasPassword: false, hasPassphrase: false, hasTotpSecret: false, transportMode: 'exec', tryKeyboard: true, enabled: true, notes: '', ...patch });
export async function feature(name) {
  try {
    return await import('../src/' + name + '.mjs');
  }
  catch (error) {
    if (error.code === 'ERR_MODULE_NOT_FOUND' && error.url?.includes('/engine/src/'))
      assert.fail('Missing engine implementation: ' + name);
    throw error;
  }
}
export async function request(engine, path, body, headers = {}) {
  if (headers.host)
    return new Promise((resolve, reject) => {
      const req = http.request(engine.url + path, { method: body === undefined ? 'GET' : 'POST', headers: { authorization: 'Bearer ' + tokens.controlToken, ...headers } }, res => {
        let text = '';
        res.on('data', chunk => text += chunk);
        res.on('end', () => resolve({ status: res.statusCode, body: JSON.parse(text), headers: res.headers }));
      });
      req.on('error', reject);
      req.end(body === undefined ? undefined : JSON.stringify(body));
    });
  const response = await fetch(engine.url + path, { method: body === undefined ? 'GET' : 'POST', headers: { authorization: 'Bearer ' + tokens.controlToken, ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  return { status: response.status, body: await response.json(), headers: response.headers };
}
export async function eventually(fn, timeout = 4000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await fn();
    if (value)
      return value;
    await new Promise(resolve => setTimeout(resolve, 15));
  }
  assert.fail('Timed out awaiting expected state');
}
export async function fakeSsh(options = {}) {
  const { privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048, privateKeyEncoding: { type: 'pkcs1', format: 'pem' }, publicKeyEncoding: { type: 'spki', format: 'pem' } });
  const clients = new Set();
  const seen = { connections: 0, commands: [], usernames: [], responses: [], disconnected: 0, shells: 0 };
  const files = new Map([['/binary', Buffer.from([0, 1, 2])]]);
  const server = new ssh2.Server({ hostKeys: [privateKey] }, client => {
    clients.add(client);
    seen.connections++;
    if (options.channelRejection) {
      const fail = client._protocol.channelOpenFail.bind(client._protocol);
      client._protocol.channelOpenFail = (sender, reason) => fail(sender, reason, options.channelRejection);
    }
    client.on('error', () => {
    });
    client.on('close', () => {
      clients.delete(client);
      seen.disconnected++;
    });
    client.on('authentication', context => {
      seen.usernames.push(context.username);
      if (options.password && context.method === 'password' && context.password === options.password)
        return context.accept();
      if (context.method !== 'keyboard-interactive')
        return context.reject(['keyboard-interactive']);
      context.prompt(options.prompts ?? [{ prompt: 'Verification code: ', echo: false }], 'AstraLink test', 'Enter current code', responses => {
        seen.responses.push(responses);
        if (responses.at(-1) === (options.code ?? '654321'))
          context.accept();
        else
          context.reject();
      });
    });
    if (options.allowForwarding)
      client.on('tcpip', (accept, reject, info) => {
        if (options.onForward) {
          options.onForward({ accept, reject, info });
          return;
        }
        if (info.destIP !== '127.0.0.1') {
          reject();
          return;
        }
        const socket = net.connect(info.destPort, info.destIP, () => {
          const stream = accept();
          socket.pipe(stream).pipe(socket);
          stream.on('close', () => socket.destroy());
        });
        socket.on('error', () => {
          reject();
          socket.destroy();
        });
      });
    client.on('ready', () => client.on('session', (accept, reject) => {
      if (options.onSession) {
        options.onSession({ accept, reject });
        return;
      }
      const session = accept();
      session.on('pty', acceptPty => acceptPty());
      if (options.sftp)
        session.on('sftp', acceptSftp => {
          const sftp = acceptSftp(), handles = new Map();
          let serial = 0;
          const { STATUS_CODE: S, OPEN_MODE: F } = ssh2.utils.sftp;
          const attrs = remote => ({ mode: 0o100644, uid: 1000, gid: 1000, size: files.get(remote).length, atime: 0, mtime: 0 });
          const stat = (id, remote) => files.has(remote) ? sftp.attrs(id, attrs(remote)) : sftp.status(id, S.NO_SUCH_FILE);
          sftp.on('STAT', stat);
          sftp.on('LSTAT', stat);
          sftp.on('FSTAT', (id, handle) => stat(id, handles.get(handle.toString())));
          sftp.on('OPEN', (id, remote, flags) => {
            if (!files.has(remote) && !(flags & F.CREAT)) {
              sftp.status(id, S.NO_SUCH_FILE);
              return;
            }
            if (!files.has(remote) || (flags & F.TRUNC))
              files.set(remote, Buffer.alloc(0));
            const handle = Buffer.from(String(++serial));
            handles.set(handle.toString(), remote);
            sftp.handle(id, handle);
          });
          sftp.on('READ', (id, handle, offset, length) => {
            const bytes = files.get(handles.get(handle.toString()));
            if (offset >= bytes.length)
              sftp.status(id, S.EOF);
            else
              sftp.data(id, bytes.subarray(offset, offset + length));
          });
          sftp.on('WRITE', (id, handle, offset, data) => {
            const remote = handles.get(handle.toString()), previous = files.get(remote), next = Buffer.alloc(Math.max(previous.length, offset + data.length));
            previous.copy(next);
            data.copy(next, offset);
            files.set(remote, next);
            sftp.status(id, S.OK);
          });
          sftp.on('CLOSE', (id, handle) => {
            handles.delete(handle.toString());
            sftp.status(id, S.OK);
          });
        });
      if (options.shell)
        session.on('shell', acceptShell => {
          const stream = acceptShell();
          seen.shells++;
          const shellDir = path.dirname(options.shell.executable);
          const shell = spawn(options.shell.executable, options.shell.args, {
            windowsHide: true,
            env: { ...process.env, PATH: shellDir + path.delimiter + path.resolve(shellDir, '../usr/bin') + path.delimiter + (process.env.PATH ?? '') },
            stdio: ['pipe', 'pipe', 'pipe'],
          });
          stream.pipe(shell.stdin);
          shell.stdout.pipe(stream);
          shell.stderr.pipe(stream.stderr);
          shell.stdin.on('error', () => {
          });
          shell.on('error', () => {
            stream.exit(1);
            stream.end();
          });
          shell.on('close', code => {
            if (!stream.destroyed) {
              stream.exit(code ?? 1);
              stream.end();
            }
          });
          stream.on('close', () => shell.kill());
        });
      session.on('exec', (acceptExec, reject, info) => {
        seen.commands.push(info.command);
        const stream = acceptExec();
        if (options.onExec) {
          options.onExec(stream, info.command);
          return;
        }
        if (info.command === 'hold') {
          options.onHold?.(stream);
          return;
        }
        stream.write(info.command === 'hostname && id' ? 'fake-host\nuid=1000(tester)\n' : 'remote output\n');
        stream.exit(info.command === 'nonzero' ? 7 : (info.command === 'hostname && id' ? (options.probeExit ?? 0) : 0));
        stream.end();
      });
    }));
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  return { port: server.address().port, seen, async close() {
      for (const client of clients)
        client.end();
      await new Promise(resolve => server.close(resolve));
    } };
}
