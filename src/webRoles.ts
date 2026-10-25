import type { WebUser } from './webBridge'

export type WebRole = NonNullable<WebUser['role']>

export const webRoles: { id: WebRole; name: string; permissions: string[] }[] = [
  { id: 'admin', name: '管理员', permissions: ['用户与角色', '设置与备份', '调用审计与统计', '连接与 MCP'] },
  { id: 'operator', name: '操作员', permissions: ['连接维护与导入', 'MCP 调用与认证'] },
  { id: 'viewer', name: '查看者', permissions: ['查看连接与运行日志'] },
]

export const webRoleName = (id: WebRole | null) => id === null ? '未分配' : webRoles.find(role => role.id === id)?.name ?? id
