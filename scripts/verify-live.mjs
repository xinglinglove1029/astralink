import { readFile, writeFile } from 'node:fs/promises';
import { randomUUID, randomBytes } from 'node:crypto';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { createEngine } from '../engine/src/engine.mjs';

const [configPath, connectionName] = process.argv.slice(2);
if (!configPath || !connectionName) throw new Error('Usage: node scripts/verify-live.mjs <ssh-config.json> <connection-name>');
const source = JSON.parse((await readFile(configPath, 'utf8')).replace(/^\uFEFF/, ''));
const profiles = Array.isArray(source) ? source : Object.entries(source).map(([name, config]) => ({ ...config, name }));
const config = profiles.find(profile => profile.name === connectionName);
if (!config) throw new Error('Requested connection is absent');
const profile = {
  id: randomUUID(), name: config.name, host: config.host, port: config.port || 22, username: config.username,
  group: '', source: 'live-check', authType: config.privateKey ? 'privateKey' : config.agent ? 'agent' : 'password',
  ...(config.privateKey ? { privateKeyPath: config.privateKey } : {}),
  ...(config.password ? { password: config.password } : {}),
  ...(config.passphrase ? { passphrase: config.passphrase } : {}),
  ...(config.agent ? { agent: config.agent } : {}),
  hasPassword: Boolean(config.password), hasPassphrase: Boolean(config.passphrase),
  transportMode: config.transportMode || 'exec', tryKeyboard: Boolean(config.tryKeyboard), enabled: true, notes: '',
};
const controlToken = randomBytes(32).toString('hex');
const mcpToken = randomBytes(32).toString('hex');
const engine = await createEngine({ port: 0, controlToken, mcpToken });
const client = new Client({ name: 'astralink-live-verification', version: '0.1.0' });
try {
  const response = await fetch(engine.url + '/control/profiles', {
    method: 'POST', headers: { authorization: `Bearer ${controlToken}`, 'content-type': 'application/json' },
    body: JSON.stringify({ profiles: [profile] }),
  });
  if (!response.ok) throw new Error('Runtime profile validation failed: ' + response.status);
  await client.connect(new StreamableHTTPClientTransport(new URL(engine.url + '/mcp'), { requestInit: { headers: { authorization: `Bearer ${mcpToken}` } } }));
  const tools = await client.listTools();
  const result = await client.callTool({ name: 'execute-command', arguments: { connectionName, cmdString: 'hostname && id && uname -s', timeout: 15000 } }, undefined, { timeout: 90000 });
  if (result.isError) throw new Error('Readonly command failed: ' + result.content.map(item => item.text || '').join('\n'));
  const summary = { connectionName, host: config.host, port: config.port || 22, toolCount: tools.tools.length, result: result.content.map(item => item.text || '').join('\n'), checkedAt: new Date().toISOString() };
  console.log(JSON.stringify(summary, null, 2));
  if (process.env.ASTRALINK_VERIFICATION_OUTPUT) await writeFile(process.env.ASTRALINK_VERIFICATION_OUTPUT, JSON.stringify(summary, null, 2) + '\n');
} finally {
  await client.close().catch(() => {});
  await engine.close();
}
