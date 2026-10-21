export type ClientId =
  | 'claude-code' | 'codex' | 'cursor' | 'vscode' | 'windsurf'
  | 'antigravity' | 'opencode' | 'openclaw' | 'hermes'
  | 'claude-desktop' | 'chatgpt'

export interface ClientGuide {
  id: ClientId
  name: string
  location: string
  supported: boolean
  steps: string[]
  verify: string
  caution?: string
}

export const clientGuides: ClientGuide[] = [
  { id: 'claude-code', name: 'Claude Code', location: '用户级 ~/.claude.json；项目级 .mcp.json', supported: true,
    steps: ['一键接入写入用户级配置；也可将片段并入项目 .mcp.json。', '粘贴 星枢 Astralink 访问令牌，重新启动 Claude Code。'], verify: '运行 claude mcp list；在会话中检查 /mcp。', caution: '项目配置可能随代码提交；不要提交含真实令牌的文件。' },
  { id: 'codex', name: 'Codex', location: '用户目录 ~/.codex/config.toml', supported: true,
    steps: ['优先使用下方的一键接入；手动配置时，将片段并入 config.toml。', '填写访问令牌并重新加载 Codex。'], verify: '运行 codex mcp list，并在新会话中列出 星枢 Astralink 工具。' },
  { id: 'cursor', name: 'Cursor', location: '用户目录 ~/.cursor/mcp.json', supported: true,
    steps: ['将片段并入 mcpServers，不要覆盖已有连接。', '填写访问令牌，在 Cursor 的 MCP 设置中刷新。'], verify: '在 Cursor 的 MCP 列表中检查 星枢 Astralink 工具。' },
  { id: 'vscode', name: 'VS Code', location: '工作区 .vscode/mcp.json 或用户 MCP 配置', supported: true,
    steps: ['将片段并入配置；首次连接时 VS Code 会提示输入令牌。', '从 星枢 Astralink 复制访问令牌并粘贴到凭据提示框。'], verify: '执行 MCP: List Servers，检查 星枢 Astralink 是否已连接。' },
  { id: 'windsurf', name: 'Windsurf', location: '旧版 Cascade: %APPDATA%/devin/mcp_config.json', supported: true,
    steps: ['仅适用于旧版 Cascade：在其 MCP 菜单中打开原始配置。', '将片段并入 mcpServers，填写访问令牌并刷新。'], verify: '在 Cascade MCP 列表检查工具数量。', caution: '新版 Devin Local 使用自己的 MCP 配置，不能直接套用此片段。' },
  { id: 'antigravity', name: 'Antigravity', location: '~/.gemini/config/mcp_config.json', supported: true,
    steps: ['在 MCP Servers 菜单打开原始配置。', '将片段并入 mcpServers，填写访问令牌并刷新。'], verify: '在 MCP Servers 查看 星枢 Astralink 的连接和工具。' },
  { id: 'opencode', name: 'OpenCode', location: '用户级 ~/.config/opencode/opencode.json 或项目配置', supported: true,
    steps: ['将片段并入 mcp 对象，填写访问令牌。', '重新启动本机 OpenCode 会话。'], verify: '运行 opencode mcp list。' },
  { id: 'openclaw', name: 'OpenClaw', location: '设置 > MCP；高级字段使用配置编辑器', supported: true,
    steps: ['在 OpenClaw 的 MCP 配置编辑器中合并片段。', '填写访问令牌，确保 Gateway 与 星枢 Astralink 在同一台机器。'], verify: '运行 openclaw mcp doctor astralink --probe。', caution: '远程 Gateway 中的 127.0.0.1 是 Gateway 自己，不是运行 星枢 Astralink 的电脑。' },
  { id: 'hermes', name: 'Hermes', location: '~/.hermes/config.yaml', supported: true,
    steps: ['将片段合并到 mcp_servers，填写访问令牌。', '在同一台电脑上启动 Hermes。'], verify: '在 Hermes 新会话中查看 MCP 工具。' },
  { id: 'claude-desktop', name: 'Claude Desktop', location: '当前本机 HTTP 接入未验证', supported: false,
    steps: ['目前不生成可能失效的 localhost 配置。', '需先实现并验证 Claude Desktop 对本机 HTTP 或 stdio 桥接的接入。'], verify: '该客户端尚未通过 星枢 Astralink 端到端验收。' },
  { id: 'chatgpt', name: 'ChatGPT', location: '网页版无法直接访问本机环回地址', supported: false,
    steps: ['当前 星枢 Astralink 只监听 127.0.0.1，云端 ChatGPT 无法直接回连。', '若将来接入，应另行设计经认证的 HTTPS 网关并评估远程暴露风险。'], verify: '当前不提供可用的本机配置片段。' },
]

const token = '<ASTRALINK_MCP_TOKEN>'
const header = { Authorization: 'Bearer ' + token }
const pretty = (data: unknown) => JSON.stringify(data, null, 2)

export function mcpEndpoint(port: number): string {
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('MCP 端口无效')
  return 'http://127.0.0.1:' + port + '/mcp'
}

export function createClientSnippet(client: ClientId, port: number): string | null {
  const url = mcpEndpoint(port)
  switch (client) {
    case 'claude-code': return pretty({ mcpServers: { astralink: { type: 'http', url, headers: header } } })
    case 'codex': return '[mcp_servers.astralink]\nenabled = true\nurl = "' + url + '"\n\n[mcp_servers.astralink.http_headers]\nAuthorization = "Bearer ' + token + '"'
    case 'cursor': return pretty({ mcpServers: { astralink: { url, headers: header } } })
    case 'vscode': return pretty({ servers: { astralink: { type: 'http', url, headers: { Authorization: 'Bearer $' + '{input:astralink-token}' } } }, inputs: [{ id: 'astralink-token', type: 'promptString', description: '星枢 Astralink MCP 访问令牌', password: true }] })
    case 'windsurf': return pretty({ mcpServers: { astralink: { serverUrl: url, headers: header } } })
    case 'antigravity': return pretty({ mcpServers: { astralink: { serverUrl: url, headers: header } } })
    case 'opencode': return pretty({ mcp: { astralink: { type: 'remote', url, enabled: true, oauth: false, headers: header } } })
    case 'openclaw': return pretty({ mcp: { servers: { astralink: { url, transport: 'streamable-http', enabled: true, headers: header } } } })
    case 'hermes': return 'mcp_servers:\n  astralink:\n    url: "' + url + '"\n    headers:\n      Authorization: "Bearer ' + token + '"'
    case 'claude-desktop':
    case 'chatgpt': return null
  }
}

export async function copyAccessToken(
  read: () => Promise<string>,
  write: (value: string) => Promise<void>,
): Promise<void> {
  const token = await read()
  if (!token) throw new Error('MCP 访问令牌不可用')
  await write(token)
}
