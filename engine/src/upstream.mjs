import { AsyncLocalStorage } from 'node:async_hooks';
import { SSHConnectionManager } from '@fangjunjie/ssh-mcp-server/build/services/ssh-connection-manager.js';
import { registerAllTools } from '@fangjunjie/ssh-mcp-server/build/tools/index.js';
import { Logger } from '@fangjunjie/ssh-mcp-server/build/utils/logger.js';
import { redact } from './errors.mjs';
export { SSHConnectionManager };
// These internal entrypoints are present in the pinned 1.10.0 resource tree.
// Scope upstream's singleton per tool call, without changing its registrations.
const context = new AsyncLocalStorage();
const managerFacade = new Proxy({}, { get(_target, key) {
    return (...args) => {
      const pool = context.getStore()?.pool;
      if (!pool)
        throw new Error('Missing engine tool context');
      return pool[key](...args);
    };
  } });
// Keep credentials for each operation generation until its result is sanitized.
export function captureOperationSecrets(generation, profiles) {
  const scope = context.getStore();
  if (!scope || scope.generations.has(generation))
    return;
  scope.generations.add(generation);
  for (const profile of profiles) {
    for (const secret of [profile.password, profile.passphrase, profile.totpSecret])
      if (typeof secret === 'string' && secret.length)
        scope.secrets.add(secret);
  }
}
SSHConnectionManager.getInstance = () => managerFacade;
// Upstream logs server prompts, arbitrary commands and exception text. The
// embedded engine deliberately emits no such diagnostics or credential data.
Logger.log = () => {
};
Logger.handleError = () => '操作失败';
export function registerUpstreamTools(server, pool) {
  registerAllTools({ registerTool(name, config, handler) {
      server.registerTool(name, config, (...args) => {
        const scope = { pool, secrets: new Set(pool.secrets()), generations: new Set() };
        const secrets = () => {
          const values = [...scope.secrets, ...pool.secrets()];
          // Upstream formats diagnostic messages as JSON text. Include escaped forms.
          return [...values, ...values.map(value => JSON.stringify(value).slice(1, -1))];
        };
        return context.run(scope, async () => {
          pool.diagnostics?.record('tool_started');
          try {
            const result = await handler(...args);
            if (result?.isError)
            {
              pool.diagnostics?.record('tool_failed');
              return { ...result, content: result.content.map(item => item.type === 'text' ? { ...item, text: redact(item.text, secrets()) } : item) };
            }
            pool.diagnostics?.record('tool_finished');
            return result;
          }
          catch (error) {
            pool.diagnostics?.record('tool_failed');
            if (error instanceof Error) {
              error.message = redact(error.message, secrets());
              throw error;
            }
            throw new Error(redact(String(error), secrets()));
          }
          finally {
            scope.secrets.clear();
            scope.generations.clear();
          }
        });
      });
    } });
}
