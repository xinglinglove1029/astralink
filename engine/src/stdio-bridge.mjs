import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';

const port = Number(process.env.ASTRALINK_MCP_PORT);
const token = process.env.ASTRALINK_MCP_TOKEN;
if (!Number.isInteger(port) || port < 1 || port > 65535 || !token || /[\r\n]/.test(token)) {
  console.error('AstraLink bridge configuration is invalid');
  process.exit(1);
}

const client = new Client({ name: 'astralink-local-bridge', version: '0.1.1' });
const remote = new StreamableHTTPClientTransport(new URL(`http://127.0.0.1:${port}/mcp`), {
  requestInit: { headers: { Authorization: `Bearer ${token}` } },
});
const server = new Server({ name: 'astralink', version: '0.1.1' }, { capabilities: { tools: {} } });
server.setRequestHandler(ListToolsRequestSchema, async () => client.listTools());
server.setRequestHandler(CallToolRequestSchema, async (request) => client.callTool(request.params));

try {
  await client.connect(remote);
  await server.connect(new StdioServerTransport());
} catch {
  console.error('AstraLink bridge could not connect to the local MCP service');
  await client.close().catch(() => {});
  process.exitCode = 1;
}

process.stdin.on('end', () => {
  void Promise.allSettled([server.close(), client.close()]);
});
