import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = readFileSync(join(root, 'src/assets/astralink-mark.svg'), 'utf8');
const mark = source.match(/<svg\b[^>]*>([\s\S]*)<\/svg>\s*$/)?.[1];
if (!mark || !source.includes('viewBox="0 0 64 64"')) {
  throw new Error('图形源稿必须使用 64 × 64 SVG 画布');
}

// Keep one geometry source for the interface, favicon and native desktop icons.
const icon = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512" fill="none">
  <defs>
    <linearGradient id="tile" x1="74" y1="32" x2="432" y2="488" gradientUnits="userSpaceOnUse">
      <stop stop-color="#173e50"/>
      <stop offset="0.55" stop-color="#102a3b"/>
      <stop offset="1" stop-color="#0b1c2d"/>
    </linearGradient>
    <linearGradient id="edge" x1="96" y1="24" x2="400" y2="488" gradientUnits="userSpaceOnUse">
      <stop stop-color="#72b7b5" stop-opacity="0.5"/>
      <stop offset="0.5" stop-color="#47777e" stop-opacity="0.12"/>
      <stop offset="1" stop-color="#234353" stop-opacity="0.4"/>
    </linearGradient>
  </defs>
  <rect x="24" y="24" width="464" height="464" rx="112" fill="url(#tile)"/>
  <rect x="25.5" y="25.5" width="461" height="461" rx="110.5" stroke="url(#edge)" stroke-width="3"/>
  <g transform="translate(32 32) scale(7)">${mark}  </g>
</svg>
`;
const iconPath = join(root, 'src/assets/astralink-icon.svg');
writeFileSync(iconPath, icon);

const temporary = mkdtempSync(join(tmpdir(), 'astralink-icons-'));
try {
  const result = spawnSync(process.execPath, [
    join(root, 'node_modules/@tauri-apps/cli/tauri.js'),
    'icon', iconPath, '--output', temporary,
  ], { cwd: root, encoding: 'utf8', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`图标生成失败：${result.status}\n${result.stderr || result.stdout}`);
  // Mobile outputs are outside this desktop application's packaging scope.
  for (const name of ['32x32.png', '64x64.png', '128x128.png', '128x128@2x.png', 'icon.png', 'icon.ico', 'icon.icns']) {
    copyFileSync(join(temporary, name), join(root, 'src-tauri/icons', name));
  }
  console.log('Astralink 桌面图标已更新');
} finally {
  if (dirname(resolve(temporary)) !== resolve(tmpdir()) || !basename(temporary).startsWith('astralink-icons-')) {
    throw new Error('拒绝清理图标临时目录之外的路径');
  }
  rmSync(temporary, { recursive: true, force: true });
}
