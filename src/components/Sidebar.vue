<script setup lang="ts">
import Icon from './Icon.vue'
import NexusMark from './NexusMark.vue'
import type { EngineStatus } from '../types'
defineProps<{ page: string; group: string; groups: { name: string; count: number }[]; total: number; engine: EngineStatus; desktop: boolean }>()
const emit = defineEmits<{ navigate: [page: string]; group: [group: string] }>()
</script>
<template>
  <aside class="sidebar">
    <a class="brand" href="#" aria-label="星枢 Astralink 首页" @click.prevent="emit('navigate', 'dashboard')"><span class="brand-mark"><NexusMark :size="31"/></span><span class="brand-wordmark"><strong>星枢</strong><small>ASTRALINK</small></span></a>
    <div class="sidebar-label">工作空间 / WORKSPACE</div>
    <nav aria-label="主导航">
      <button :class="['nav-item', { active: page === 'dashboard' }]" @click="emit('navigate', 'dashboard')"><Icon name="globe"/><span>连接总览<small>OVERVIEW</small></span></button>
      <button :class="['nav-item', { active: page === 'servers' }]" @click="emit('navigate', 'servers'); emit('group', '')"><Icon name="grid" /><span>服务器<small>NODES</small></span><span class="nav-count">{{ total }}</span></button>
      <button :class="['nav-item', { active: page === 'import' }]" @click="emit('navigate', 'import')"><Icon name="download" /><span>导入连接<small>IMPORT</small></span></button>
      <button :class="['nav-item', { active: page === 'logs' }]" @click="emit('navigate', 'logs')"><Icon name="logs" /><span>运行日志<small>ACTIVITY</small></span></button>
      <button :class="['nav-item', { active: page === 'settings' }]" @click="emit('navigate', 'settings')"><Icon name="settings" /><span>设置<small>SETTINGS</small></span></button>
    </nav>
    <div class="sidebar-label groups-heading">服务器分组<span>{{ groups.length }}</span></div>
    <div class="group-list">
      <button :class="['group-item', { selected: page === 'servers' && !group }]" @click="emit('navigate', 'servers'); emit('group', '')"><span class="group-dot all" /><span>全部服务器</span><span>{{ total }}</span></button>
      <button v-for="(item, index) in groups" :key="item.name" :class="['group-item', { selected: page === 'servers' && group === item.name }]" @click="emit('navigate', 'servers'); emit('group', item.name)"><span class="group-dot" :style="{ background: ['#60a5fa', '#a78bfa', '#34d399', '#fbbf24'][index % 4] }" /><span>{{ item.name }}</span><span>{{ item.count }}</span></button>
      <p v-if="!groups.length" class="sidebar-empty">创建连接时添加分组</p>
    </div>
    <div class="sidebar-bottom">
      <div class="sidebar-service"><span :class="['status-dot', { live: desktop && engine.running }]" /><span>{{ !desktop ? '桌面应用未连接' : engine.running ? 'MCP 服务运行中' : 'MCP 服务未启动' }}</span></div>
      <p>本机运行 · SSH 连接由你掌控</p>
      <div class="sidebar-version"><span class="mini-mark"><NexusMark :size="19"/></span><span>星枢 <small>ASTRALINK · v0.1.0</small></span><Icon name="shield" :size="16" /></div>
    </div>
  </aside>
</template>
