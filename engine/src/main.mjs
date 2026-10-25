import { pathToFileURL } from 'node:url';
import { createEngine } from './engine.mjs';
export async function main(env = process.env) {
  if (Number(process.versions.node.split('.')[0]) < 24)
    throw new Error('需要 Node 24 或更新版本');
  const value = env.ASTRALINK_PORT ?? '18766';
  if (!/^[0-9]+$/.test(value) || Number(value) < 1 || Number(value) > 65535)
    throw new Error('端口无效');
  const engine = await createEngine({ port: Number(value), controlToken: env.ASTRALINK_CONTROL_TOKEN, mcpToken: env.ASTRALINK_MCP_TOKEN,
    identitiesFile: env.ASTRALINK_MCP_IDENTITIES_FILE, auditFile: env.ASTRALINK_MCP_AUDIT_FILE, remoteHost: env.ASTRALINK_MCP_REMOTE_HOST });
  const stop = () => void engine.close();
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  engine.closed.then(() => {
    process.removeListener('SIGINT', stop);
    process.removeListener('SIGTERM', stop);
  });
  return engine;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => {
    process.stderr.write('AstraLink 引擎启动失败，请检查运行环境、端口和访问令牌。\n');
    process.exitCode = 1;
  });
}
