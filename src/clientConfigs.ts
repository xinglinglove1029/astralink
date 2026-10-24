export type ClientId =
  | 'claude-code' | 'codex' | 'cursor' | 'vscode' | 'windsurf'
  | 'antigravity' | 'opencode' | 'openclaw' | 'hermes'
  | 'claude-desktop' | 'chatgpt' | 'workbuddy' | 'cherry-studio'

export interface ClientGuide {
  id: ClientId
  name: string
  location: string
  supported: boolean
  quickSetup?: boolean
  steps: string[]
  verify: string
  caution?: string
}

export const clientGuides: ClientGuide[] = [
  { id: 'claude-code', name: 'Claude Code', location: '用户级 ~/.claude.json；项目级 .mcp.json', supported: true,
    steps: ['一键接入写入用户级配置；也可将片段并入项目 .mcp.json。', '粘贴 AstraLink 访问令牌，重新启动 Claude Code。'], verify: '运行 claude mcp list；在会话中检查 /mcp。', caution: '项目配置可能随代码提交；不要提交含真实令牌的文件。' },
  { id: 'codex', name: 'Codex', location: '用户目录 ~/.codex/config.toml', supported: true,
    steps: ['优先使用下方的一键接入；手动配置时，将片段并入 config.toml。', '填写访问令牌并重新加载 Codex。'], verify: '运行 codex mcp list，并在新会话中列出 AstraLink 工具。' },
  { id: 'cursor', name: 'Cursor', location: '用户目录 ~/.cursor/mcp.json', supported: true,
    steps: ['将片段并入 mcpServers，不要覆盖已有连接。', '填写访问令牌，在 Cursor 的 MCP 设置中刷新。'], verify: '在 Cursor 的 MCP 列表中检查 AstraLink 工具。' },
  { id: 'vscode', name: 'VS Code', location: '工作区 .vscode/mcp.json 或用户 MCP 配置', supported: true,
    steps: ['将片段并入配置；首次连接时 VS Code 会提示输入令牌。', '从 AstraLink 复制访问令牌并粘贴到凭据提示框。'], verify: '执行 MCP: List Servers，检查 AstraLink 是否已连接。' },
  { id: 'windsurf', name: 'Windsurf', location: '旧版 Cascade: %APPDATA%/devin/mcp_config.json', supported: true,
    steps: ['仅适用于旧版 Cascade：在其 MCP 菜单中打开原始配置。', '将片段并入 mcpServers，填写访问令牌并刷新。'], verify: '在 Cascade MCP 列表检查工具数量。', caution: '新版 Devin Local 使用自己的 MCP 配置，不能直接套用此片段。' },
  { id: 'antigravity', name: 'Antigravity', location: '~/.gemini/config/mcp_config.json', supported: true,
    steps: ['在 MCP Servers 菜单打开原始配置。', '将片段并入 mcpServers，填写访问令牌并刷新。'], verify: '在 MCP Servers 查看 AstraLink 的连接和工具。' },
  { id: 'opencode', name: 'OpenCode', location: '用户级 ~/.config/opencode/opencode.json 或项目配置', supported: true,
    steps: ['将片段并入 mcp 对象，填写访问令牌。', '重新启动本机 OpenCode 会话。'], verify: '运行 opencode mcp list。' },
  { id: 'openclaw', name: 'OpenClaw', location: '设置 > MCP；高级字段使用配置编辑器', supported: true,
    steps: ['在 OpenClaw 的 MCP 配置编辑器中合并片段。', '填写访问令牌，确保 Gateway 与 AstraLink 在同一台机器。'], verify: '运行 openclaw mcp doctor astralink --probe。', caution: '远程 Gateway 中的 127.0.0.1 是 Gateway 自己，不是运行 AstraLink 的电脑。' },
  { id: 'hermes', name: 'Hermes', location: '~/.hermes/config.yaml', supported: true,
    steps: ['将片段合并到 mcp_servers，填写访问令牌。', '在同一台电脑上启动 Hermes。'], verify: '在 Hermes 新会话中查看 MCP 工具。' },
  { id: 'claude-desktop', name: 'Claude Desktop', location: '本机 claude_desktop_config.json', supported: true,
    steps: ['一键配置写入本机 stdio 桥接与内置 Node 路径。', '保持 AstraLink 服务运行，完全退出并重新启动 Claude Desktop。'], verify: '在 Claude Desktop 的 MCP 工具列表检查 astralink。', caution: '客户端配置保存访问令牌；仅写入本机用户配置，不要同步或提交。' },
  { id: 'chatgpt', name: 'ChatGPT', location: '云端连接器需 Secure MCP Tunnel 或公网 HTTPS 地址', supported: false,
    steps: ['本机 127.0.0.1 地址无法直接用于云端 ChatGPT。', '在支持该功能的 ChatGPT 工作空间配置 Secure MCP Tunnel，或单独部署经认证的公网 HTTPS MCP 网关。', '确认身份认证、工具权限及审计策略后，在 ChatGPT 的自定义 MCP 连接中添加。'], verify: '本机服务不会自动向公网开放；未配置隧道或网关前不能一键接入。', caution: '不要把本机访问令牌或 SSH 凭据粘贴到公开 URL。' },
  { id: 'workbuddy', name: 'WorkBuddy', location: '用户级 ~/.workbuddy/mcp.json', supported: true,
    steps: ['一键写入用户级配置，或在插件 → MCP 服务器 → 配置 MCP 中手动合并片段。', '保持 AstraLink 服务运行，刷新 WorkBuddy 的 MCP 列表。'], verify: '在 MCP 服务器列表检查 astralink 状态与工具。', caution: '项目级 .workbuddy/mcp.json 可能随代码提交；不要提交含真实令牌的文件。' },
  { id: 'cherry-studio', name: 'Cherry Studio', location: '设置 > MCP > MCP 服务器 > 添加', supported: true, quickSetup: false,
    steps: ['在 Cherry Studio 中从 JSON 导入本机 stdio 桥接配置。', '桌面版会填入内置 Node 和脚本路径；复制令牌并替换片段中的占位符。', '启用服务器后，在目标 Agent 的 MCP 设置中绑定 astralink。'], verify: '在服务器详情查看工具，再在 Agent 中调用 list-servers。', caution: '客户端配置含访问令牌，请勿同步或提交。' },
]

const token = '<ASTRALINK_MCP_TOKEN>'
const header = { Authorization: 'Bearer ' + token }
const pretty = (data: unknown) => JSON.stringify(data, null, 2)

export function mcpEndpoint(port: number, host = '127.0.0.1'): string {
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('MCP 端口无效')
  const address = host.includes(':') ? `[${host}]` : host
  return `http://${address}:${port}/mcp`
}

export function createClientSnippet(client: ClientId, port: number, bridgePaths?: { nodePath: string; scriptPath: string }, host = '127.0.0.1'): string | null {
  const url = mcpEndpoint(port, host)
  switch (client) {
    case 'claude-code': return pretty({ mcpServers: { astralink: { type: 'http', url, headers: header } } })
    case 'codex': return '[mcp_servers.astralink]\nenabled = true\nurl = "' + url + '"\n\n[mcp_servers.astralink.http_headers]\nAuthorization = "Bearer ' + token + '"'
    case 'cursor': return pretty({ mcpServers: { astralink: { url, headers: header } } })
    case 'vscode': return pretty({ servers: { astralink: { type: 'http', url, headers: { Authorization: 'Bearer $' + '{input:astralink-token}' } } }, inputs: [{ id: 'astralink-token', type: 'promptString', description: 'AstraLink MCP 访问令牌', password: true }] })
    case 'windsurf': return pretty({ mcpServers: { astralink: { serverUrl: url, headers: header } } })
    case 'antigravity': return pretty({ mcpServers: { astralink: { serverUrl: url, headers: header } } })
    case 'opencode': return pretty({ mcp: { astralink: { type: 'remote', url, enabled: true, oauth: false, headers: header } } })
    case 'openclaw': return pretty({ mcp: { servers: { astralink: { url, transport: 'streamable-http', enabled: true, headers: header } } } })
    case 'hermes': return 'mcp_servers:\n  astralink:\n    url: "' + url + '"\n    headers:\n      Authorization: "Bearer ' + token + '"'
    case 'claude-desktop':
    case 'workbuddy':
    case 'cherry-studio': return pretty({ mcpServers: { astralink: { command: bridgePaths?.nodePath ?? '<ASTRALINK_NODE_PATH>', args: [bridgePaths?.scriptPath ?? '<ASTRALINK_BRIDGE_PATH>'], env: { ASTRALINK_MCP_PORT: String(port), ASTRALINK_MCP_TOKEN: token } } } })
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
