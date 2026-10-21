import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { feature, profile, tokens, request, fakeSsh } from './helpers.mjs';
function findShell() {
  if (process.platform !== 'win32') {
    for (const executable of ['/bin/bash', '/bin/sh'])
      if (existsSync(executable))
        return executable;
    return undefined;
  }
  const git = spawnSync('where.exe', ['git'], { encoding: 'utf8', windowsHide: true }).stdout?.trim().split(/\r?\n/)[0];
  const candidates = [process.env.ASTRALINK_TEST_SHELL, ...(git ? [path.resolve(path.dirname(git), '../bin/bash.exe'), path.resolve(path.dirname(git), '../usr/bin/bash.exe')] : [])];
  return candidates.find(executable => executable && existsSync(executable));
}
test('shell-mode MCP uses a real local SSH shell and retains output, queueing, exit codes and SFTP restrictions', async (t) => {
  const executable = findShell();
  if (!executable) {
    t.skip('No local POSIX shell is installed');
    return;
  }
  const args = path.basename(executable).startsWith('bash') ? ['--noprofile', '--norc', '-s'] : ['-s'];
  const ssh = await fakeSsh({ password: 'shell-pass', shell: { executable, args } });
  t.after(() => ssh.close());
  const { createEngine } = await feature('engine');
  const engine = await createEngine({ ...tokens, port: 0 });
  t.after(() => engine.close());
  const p = profile({ port: ssh.port, password: 'shell-pass', tryKeyboard: false, transportMode: 'shell' });
  assert.equal((await request(engine, '/control/profiles', { profiles: [p] })).status, 200);
  const client = new Client({ name: 'real shell test', version: '1' });
  await client.connect(new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: 'Bearer ' + tokens.mcpToken } } }));
  t.after(() => client.close());
  const call = cmdString => client.callTool({ name: 'execute-command', arguments: { connectionName: p.name, cmdString, timeout: 5000 } });
  const success = await call("printf '%s' 'shell-ready'");
  assert.ok(!success.isError, JSON.stringify(success));
  assert.equal(success.content[0].text, 'shell-ready');
  const failure = await call("printf '%s' 'shell-failure'; false");
  assert.ok(!failure.isError);
  assert.match(failure.content[0].text, /shell-failure/);
  assert.match(failure.content[0].text, /\[exit code\] 1/);
  const [one, two] = await Promise.all([call("printf '%s' 'one'"), call("printf '%s' 'two'")]);
  assert.equal(one.content[0].text, 'one');
  assert.equal(two.content[0].text, 'two');
  const probe = await request(engine, '/control/test', { profileId: p.id });
  assert.equal(probe.body.status, 'connected');
  assert.match(probe.body.stdout, /uid=/);
  const unsupported = await client.callTool({ name: 'read-file', arguments: { connectionName: p.name, remotePath: '/example' } });
  assert.equal(unsupported.isError, true);
  assert.match(unsupported.content[0].text, /UNSUPPORTED_IN_SHELL_MODE/);
  assert.equal((await client.listTools()).tools.length, 10);
  assert.deepEqual(ssh.seen.commands, []);
  assert.equal(ssh.seen.shells, 1);
});
