import { copyFile, cp, mkdir, readFile, writeFile, stat, chmod, mkdtemp, rm, realpath } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join, relative, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';

const project = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const resources = join(project, 'src-tauri', 'resources');
const runtime = join(resources, 'runtime');
const engine = join(resources, 'engine');
const nodeBinary = process.env.ASTRALINK_NODE_BINARY || process.execPath;
let nodeInfo;
try {
  const { stdout } = await promisify(execFile)(nodeBinary, ['-p', 'JSON.stringify({ version: process.version, platform: process.platform, arch: process.arch })'], {
    windowsHide: true, timeout: 10000, env: { ...process.env, NODE_OPTIONS: '', NODE_PATH: '' },
  });
  nodeInfo = JSON.parse(stdout);
} catch { throw new Error('Node runtime is unavailable; install Node.js or correct ASTRALINK_NODE_BINARY'); }
if (nodeInfo.platform !== process.platform || nodeInfo.arch !== process.arch) throw new Error('Node runtime platform and architecture must match the native build');
let cachedRuntime;
try { cachedRuntime = JSON.parse(await readFile(join(runtime, 'runtime.json'), 'utf8')); } catch { cachedRuntime = undefined; }
await mkdir(runtime, { recursive: true });
await mkdir(engine, { recursive: true });
const runtimeBinary = join(runtime, process.platform === 'win32' ? 'node.exe' : 'node');
await copyFile(nodeBinary, runtimeBinary);
if (process.platform !== 'win32') await chmod(runtimeBinary, 0o755);
const binary = await readFile(nodeBinary);
await writeFile(join(runtime, 'runtime.json'), JSON.stringify({ ...nodeInfo, sha256: createHash('sha256').update(binary).digest('hex') }, null, 2));
let license;
const binaryDirectory = dirname(await realpath(nodeBinary));
for (const candidate of [join(binaryDirectory, 'LICENSE'), join(binaryDirectory, '..', 'LICENSE')]) {
  try { license = await readFile(candidate, 'utf8'); break; }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
}
if (!license && cachedRuntime?.version === nodeInfo.version) {
  try { license = await readFile(join(runtime, 'LICENSE.txt'), 'utf8'); } catch { license = undefined; }
}
for (let attempt = 0; !license && attempt < 3; attempt++) {
  try {
    const response = await fetch(`https://raw.githubusercontent.com/nodejs/node/${nodeInfo.version}/LICENSE`, { signal: AbortSignal.timeout(15000) });
    if (!response.ok) throw new Error(`Node license retrieval failed: ${response.status}`);
    license = await response.text();
  } catch (error) {
    if (attempt === 2) throw error;
    await new Promise((resume) => setTimeout(resume, 500));
  }
}
await writeFile(join(runtime, 'LICENSE.txt'), license);
await copyFile(join(project, 'engine', 'package.json'), join(engine, 'package.json'));
try { await stat(join(project, 'engine', 'src')); await cp(join(project, 'engine', 'src'), join(engine, 'src'), { recursive: true }); }
catch (error) { if (error.code !== 'ENOENT') throw error; await mkdir(join(engine, 'src'), { recursive: true }); }
const npmCli = process.env.npm_execpath || join(dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
// Install only the engine workspace from the project lock, preserving overrides.
// A fresh CI runner has tarballs after npm ci, but may lack registry metadata for
// an unlocked npm install --offline. npm ci avoids that metadata dependency.
const installRoot = await mkdtemp(join(tmpdir(), 'astralink-runtime-'));
try {
  await copyFile(join(project, 'package.json'), join(installRoot, 'package.json'));
  await copyFile(join(project, 'package-lock.json'), join(installRoot, 'package-lock.json'));
  await mkdir(join(installRoot, 'engine'));
  await copyFile(join(project, 'engine/package.json'), join(installRoot, 'engine/package.json'));
  await new Promise((resolveInstall, reject) => {
    const child = spawn(process.execPath, [npmCli, 'ci', '--workspace=@astralink/engine', '--include-workspace-root=false', '--omit=dev', '--ignore-scripts', '--no-fund', '--no-audit', '--prefer-offline'], { cwd: installRoot, stdio: 'inherit', windowsHide: true });
    child.on('error', reject);
    child.on('exit', code => code === 0 ? resolveInstall() : reject(new Error(`Runtime dependencies failed: ${code}`)));
  });
  const modules = join(engine, 'node_modules');
  const actualParent = await realpath(engine);
  const actualProject = await realpath(project);
  const parentRelative = relative(actualProject, actualParent);
  if (parentRelative !== ['src-tauri', 'resources', 'engine'].join(sep)) throw new Error('Unexpected engine resource path');
  await rm(modules, { recursive: true, force: true });
  const workspaceLink = join(installRoot, 'node_modules', '@astralink', 'engine');
  await cp(join(installRoot, 'node_modules'), modules, { recursive: true, verbatimSymlinks: true, filter: (source) => source !== workspaceLink });
} finally {
  // mkdtemp created this exact directory under the OS temp root.
  if (dirname(installRoot) !== resolve(tmpdir())) throw new Error('Unexpected runtime staging path');
  await rm(installRoot, { recursive: true, force: true });
}
const upstreamLicense = `ISC License\n\nCopyright (c) 2025 junki.cn\n\nPermission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.\n\nTHE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.\n`;
await writeFile(join(engine, 'SSH-MCP-LICENSE.txt'), upstreamLicense);
console.log('Astralink runtime resources prepared.');
