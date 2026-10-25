<script setup lang="ts">
import Icon from './Icon.vue'
import NexusMark from './NexusMark.vue'
import type { EngineStatus } from '../types'
import { version as appVersion } from '../../package.json'
import { webMode, currentWebUser } from '../webBridge'
defineProps<{ page: string; group: string; groups: { name: string; count: number }[]; total: number; engine: EngineStatus; desktop: boolean }>()
const emit = defineEmits<{ navigate: [page: string]; group: [group: string] }>()
</script>
<template>
  <aside class="sidebar">
    <a class="brand" href="#" aria-label="AstraLink 首页" @click.prevent="emit('navigate', 'dashboard')"><span class="brand-mark"><NexusMark :size="31"/></span><span class="brand-wordmark"><strong>AstraLink</strong><small>SSH · MCP</small></span></a>
    <div class="sidebar-label">工作空间 / WORKSPACE</div>
    <nav aria-label="主导航">
      <button :class="['nav-item', { active: page === 'dashboard' }]" aria-label="连接总览" title="连接总览" @click="emit('navigate', 'dashboard')"><Icon name="globe"/><span>连接总览<small>OVERVIEW</small></span></button>
      <button v-if="webMode && currentWebUser?.role === 'admin'" :class="['nav-item', { active: page === 'analytics' }]" aria-label="统计大屏" title="统计大屏" @click="emit('navigate', 'analytics')"><Icon name="grid" /><span>统计大屏<small>ANALYTICS</small></span></button>
      <button :class="['nav-item', { active: page === 'logs' }]" aria-label="运行日志" title="运行日志" @click="emit('navigate', 'logs')"><Icon name="logs" /><span>运行日志<small>ACTIVITY</small></span></button>
      <button v-if="webMode && currentWebUser?.role === 'admin'" :class="['nav-item', { active: page === 'audit' }]" aria-label="调用审计" title="调用审计" @click="emit('navigate', 'audit')"><Icon name="logs" /><span>调用审计<small>AUDIT</small></span></button>
      <button :class="['nav-item', { active: page === 'servers' }]" aria-label="服务器" title="服务器" @click="emit('navigate', 'servers'); emit('group', '')"><Icon name="grid" /><span>服务器<small>NODES</small></span><span class="nav-count">{{ total }}</span></button>
      <button v-if="!webMode || currentWebUser?.role === 'admin' || currentWebUser?.role === 'operator'" :class="['nav-item', { active: page === 'import' }]" aria-label="导入连接" title="导入连接" @click="emit('navigate', 'import')"><Icon name="download" /><span>导入连接<small>IMPORT</small></span></button>
      <button v-if="webMode && currentWebUser?.role === 'admin'" :class="['nav-item', { active: page === 'users' }]" aria-label="用户管理" title="用户管理" @click="emit('navigate', 'users')"><Icon name="shield" /><span>用户管理<small>USERS</small></span></button>
      <button v-if="webMode && currentWebUser?.role === 'admin'" :class="['nav-item', { active: page === 'roles' }]" aria-label="角色管理" title="角色管理" @click="emit('navigate', 'roles')"><Icon name="key" /><span>角色管理<small>ROLES</small></span></button>
    </nav>
    <div class="sidebar-label groups-heading">服务器分组<span>{{ groups.length }}</span></div>
    <div class="group-list">
      <button :class="['group-item', { selected: page === 'servers' && !group }]" @click="emit('navigate', 'servers'); emit('group', '')"><span class="group-dot all" /><span>全部服务器</span><span>{{ total }}</span></button>
      <button v-for="(item, index) in groups" :key="item.name" :class="['group-item', { selected: page === 'servers' && group === item.name }]" @click="emit('navigate', 'servers'); emit('group', item.name)"><span class="group-dot" :style="{ background: ['#60a5fa', '#a78bfa', '#34d399', '#fbbf24'][index % 4] }" /><span>{{ item.name }}</span><span>{{ item.count }}</span></button>
      <p v-if="!groups.length" class="sidebar-empty">创建连接时添加分组</p>
    </div>
    <div class="sidebar-bottom">
      <div class="sidebar-service"><span :class="['status-dot', { live: desktop && engine.running }]" /><span>{{ !desktop ? '桌面应用未连接' : engine.running ? 'MCP 服务运行中' : 'MCP 服务未启动' }}</span></div>
      <p>{{ webMode ? '服务器运行 · 登录后管理连接' : '本机运行 · SSH 连接由你掌控' }}</p>
      <div class="sidebar-version"><span class="mini-mark"><NexusMark :size="19"/></span><span>AstraLink <small>v{{ appVersion }}</small></span><button :class="['sidebar-settings', { active: page === 'settings' }]" type="button" aria-label="设置" title="设置" :aria-current="page === 'settings' ? 'page' : undefined" @click="emit('navigate', 'settings')"><Icon name="settings" :size="18" /></button></div>
    </div>
  </aside>
</template>
<style scoped>
.sidebar-settings{width:34px;height:34px;flex:0 0 34px;display:grid;place-items:center;margin-left:auto;border:1px solid transparent;border-radius:6px;color:var(--sidebar-muted);transition:background-color .18s,color .18s,border-color .18s}
.sidebar-settings svg{margin:0;color:inherit}
.sidebar-settings:hover,.sidebar-settings.active{background:#6edbc514;color:#a5ecdf;border-color:#78dcca35}
@media(max-width:720px){
  .sidebar .sidebar-bottom{display:block;margin-top:auto;padding:0}
  .sidebar-bottom>.sidebar-service,.sidebar-bottom>p,.sidebar-version>.mini-mark,.sidebar-version>span:nth-child(2){display:none}
  .sidebar .sidebar-version{border:0;padding:0;justify-content:center}
  .sidebar-settings{width:44px;height:44px;flex-basis:44px;margin:0}
}
</style>
