import { describe, expect, it } from 'vitest'
import { clientGuides, copyAccessToken, createClientSnippet, mcpEndpoint } from '../clientConfigs'

describe('MCP client connection guides', () => {
  it('covers the reference clients without claiming unsupported clients are ready', () => {
    expect(clientGuides.map((guide) => guide.id)).toEqual([
      'claude-code', 'codex', 'cursor', 'vscode', 'windsurf', 'antigravity',
      'opencode', 'openclaw', 'hermes', 'claude-desktop', 'chatgpt', 'workbuddy', 'cherry-studio',
    ])
    expect(clientGuides.filter((guide) => !guide.supported).map((guide) => guide.id))
      .toEqual(['chatgpt'])
    expect(createClientSnippet('chatgpt', 18766)).toBeNull()
    expect(JSON.parse(createClientSnippet('claude-desktop', 18766)!).mcpServers.astralink.env.ASTRALINK_MCP_PORT).toBe('18766')
    expect(JSON.parse(createClientSnippet('workbuddy', 18766)!).mcpServers.astralink.env.ASTRALINK_MCP_PORT).toBe('18766')
    expect(clientGuides.find((guide) => guide.id === 'cherry-studio')?.quickSetup).toBe(false)
    const cherry = JSON.parse(createClientSnippet('cherry-studio', 18766)!).mcpServers.astralink
    expect(cherry.command).toBe('<ASTRALINK_NODE_PATH>')
    expect(cherry.args).toEqual(['<ASTRALINK_BRIDGE_PATH>'])
    expect(cherry.env.ASTRALINK_MCP_TOKEN).toBe('<ASTRALINK_MCP_TOKEN>')
    const resolved = JSON.parse(createClientSnippet('cherry-studio', 18766, { nodePath: 'C:/Astralink/node.exe', scriptPath: 'C:/Astralink/stdio-bridge.mjs' })!).mcpServers.astralink
    expect(resolved.command).toBe('C:/Astralink/node.exe')
    expect(resolved.args).toEqual(['C:/Astralink/stdio-bridge.mjs'])
    expect(resolved.env.ASTRALINK_MCP_TOKEN).toBe('<ASTRALINK_MCP_TOKEN>')
  })

  it('uses the configured loopback endpoint and only a token placeholder', () => {
    expect(mcpEndpoint(23456)).toBe('http://127.0.0.1:23456/mcp')
    const codex = createClientSnippet('codex', 23456)!
    expect(codex).toContain('http://127.0.0.1:23456/mcp')
    expect(codex).toContain('Bearer <ASTRALINK_MCP_TOKEN>')
    expect(codex).toContain('[mcp_servers.astralink.http_headers]')
    expect(codex).not.toContain('18766')
  })

  it('uses each supported client config structure', () => {
    expect(JSON.parse(createClientSnippet('claude-code', 18766)!).mcpServers.astralink.type).toBe('http')
    expect(JSON.parse(createClientSnippet('cursor', 18766)!).mcpServers.astralink.url).toContain('/mcp')
    const vscode = JSON.parse(createClientSnippet('vscode', 18766)!)
    expect(vscode.servers.astralink.type).toBe('http')
    expect(vscode.servers.astralink.headers.Authorization).toBe('Bearer ' + '$' + '{input:astralink-token}')
    expect(vscode.inputs[0].password).toBe(true)
    expect(JSON.parse(createClientSnippet('windsurf', 18766)!).mcpServers.astralink.serverUrl).toContain('/mcp')
    expect(JSON.parse(createClientSnippet('antigravity', 18766)!).mcpServers.astralink.serverUrl).toContain('/mcp')
    expect(JSON.parse(createClientSnippet('opencode', 18766)!).mcp.astralink.type).toBe('remote')
    expect(JSON.parse(createClientSnippet('openclaw', 18766)!).mcp.servers.astralink.transport).toBe('streamable-http')
    expect(createClientSnippet('hermes', 18766)).toContain('mcp_servers:')
  })

  it('refuses invalid ports', () => {
    expect(() => mcpEndpoint(0)).toThrow()
    expect(() => mcpEndpoint(65536)).toThrow()
    expect(() => mcpEndpoint(18766.5)).toThrow()
  })

  it('retrieves a secret only on an explicit copy action and never returns its value', async () => {
    let reads = 0
    const copied: string[] = []
    const read = async () => { reads++; return 'fixture-secret' }
    const write = async (value: string) => { copied.push(value) }
    expect(reads).toBe(0)
    await expect(copyAccessToken(read, write)).resolves.toBeUndefined()
    expect(reads).toBe(1)
    expect(copied).toEqual(['fixture-secret'])
  })
})
