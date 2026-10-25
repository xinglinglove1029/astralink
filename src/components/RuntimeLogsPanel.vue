<script setup lang="ts">
import { copyText } from '../clipboard'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import type { AstraLinkController } from '../controller'
import { filterRuntimeLogs, formatRuntimeLogs } from '../runtimeLogs'
import Icon from './Icon.vue'
const props = defineProps<{ dock: AstraLinkController }>()
const { state } = props.dock
const level = ref('')
const profileId = ref('')
const query = ref('')
const automatic = ref(true)
const copyState = ref('')
const logs = computed(() => filterRuntimeLogs(state.runtimeLogs.entries, level.value, profileId.value, query.value))
const available = computed(() => state.desktop && state.ready)
let timer: ReturnType<typeof setInterval> | undefined
function profileName(id?: string) { return id ? state.profiles.find(profile => profile.id === id)?.name ?? id.slice(0, 8) : '应用' }
async function copy() {
  try { await copyText(formatRuntimeLogs(logs.value.slice().reverse())); copyState.value = '已复制筛选结果' }
  catch { copyState.value = '复制失败，请使用导出' }
}
watch([level, profileId, query], () => { copyState.value = '' })
watch(() => state.ready, ready => { if (ready && state.desktop) void props.dock.refreshRuntimeLogs() })
onMounted(() => {
  if (available.value) void props.dock.refreshRuntimeLogs()
  timer = setInterval(() => { if (automatic.value && available.value) void props.dock.refreshRuntimeLogs() }, 1500)
})
onUnmounted(() => { if (timer) clearInterval(timer) })
</script>
<template>
  <section class="card runtime-logs-panel">
    <div class="log-toolbar">
      <label class="search-box"><Icon name="search" :size="16" /><input v-model="query" aria-label="搜索运行日志" placeholder="搜索阶段、原因或关联编号" /></label>
      <select v-model="level" aria-label="日志级别"><option value="">全部级别</option><option value="info">信息</option><option value="warn">警告</option><option value="error">错误</option></select>
      <select v-model="profileId" aria-label="日志服务器"><option value="">全部服务器</option><option v-for="profile in state.profiles" :key="profile.id" :value="profile.id">{{ profile.name }}</option></select>
      <label class="log-refresh-toggle"><input v-model="automatic" type="checkbox" />自动刷新</label>
      <button class="button secondary" :disabled="!available || state.logsBusy" @click="dock.refreshRuntimeLogs()"><Icon name="refresh" :size="15" />刷新</button>
      <button class="button secondary" :disabled="!available || !logs.length" @click="copy"><Icon name="copy" :size="15" />复制</button>
      <button class="button primary" :disabled="!available || state.logsExportBusy || !state.runtimeLogs.entries.length" @click="dock.exportRuntimeLogs()"><Icon name="download" :size="15" />{{ state.logsExportBusy ? '导出中…' : '导出最近日志' }}</button>
    </div>
    <div class="log-summary"><span>{{ logs.length }} 条记录 · 最新在前 · 最多显示最近 1000 条</span><span v-if="copyState" role="status">{{ copyState }}</span><span v-else-if="state.logsBusy">刷新中…</span></div>
    <p v-if="state.runtimeLogs.error" class="log-warning" role="alert">{{ state.runtimeLogs.error }}</p>
    <p v-if="!state.desktop" class="log-empty">请在桌面应用中查看运行日志</p>
    <p v-else-if="!logs.length" class="log-empty">{{ state.logsBusy ? '正在读取日志…' : query || level || profileId ? '没有匹配的日志' : '暂无运行日志；启动 MCP 或测试连接后会在此显示' }}</p>
    <div v-else class="log-list" role="log" aria-label="软件运行日志">
      <article v-for="entry in logs" :key="entry.id" class="log-entry">
        <div class="log-entry-meta"><time :datetime="new Date(entry.timestamp).toISOString()">{{ new Date(entry.timestamp).toLocaleString() }}</time><span :class="['badge', entry.level === 'error' ? 'red' : entry.level === 'warn' ? 'amber' : 'blue']">{{ entry.level === 'error' ? '错误' : entry.level === 'warn' ? '警告' : '信息' }}</span><span>{{ profileName(entry.profileId) }}</span><code>{{ entry.source }} · {{ entry.code }}</code></div>
        <p>{{ entry.message }}</p>
        <small v-if="entry.requestId" class="log-correlation">关联编号 <code>{{ entry.requestId }}</code></small>
      </article>
    </div>
    <footer class="log-footer"><p>日志文件：<span class="mono break-text">{{ state.runtimeLogs.logPath || '等待读取' }}</span></p><p>本机保留当前文件及两份历史日志。只记录运行阶段，不记录密码、密钥、验证码和命令正文。</p></footer>
  </section>
</template>
