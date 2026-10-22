<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { desktopBridge } from './api'
import { getVersion } from '@tauri-apps/api/app'
import { openUrl } from '@tauri-apps/plugin-opener'
import { checkLatestRelease, RELEASE_PAGE } from './updateCheck'
import type { UpdateResult } from './updateCheck'
import { createAstralink } from './controller'
import { authLabels, filterProfiles, needsCredential, sourceLabels } from './domain'
import type { ConnectionProfile, SourceKind } from './types'
import Icon from './components/Icon.vue'
import Sidebar from './components/Sidebar.vue'
import ServerTable from './components/ServerTable.vue'
import ConnectionPanel from './components/ConnectionPanel.vue'
import HostEditor from './components/HostEditor.vue'
import ImportPanel from './components/ImportPanel.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import RuntimeLogsPanel from './components/RuntimeLogsPanel.vue'
import DashboardPanel from './components/DashboardPanel.vue'
import ThemeSwitcher from './components/ThemeSwitcher.vue'
import NexusMark from './components/NexusMark.vue'
import OtpModal from './components/OtpModal.vue'
import Modal from './components/Modal.vue'
const dock = createAstralink(desktopBridge)
const { state } = dock
const page = ref('dashboard')
const query = ref('')
const group = ref('')
const source = ref('')
const auth = ref('')
const selectedId = ref('')
const editor = ref<{ profile?: ConnectionProfile } | null>(null)
const deleting = ref<ConnectionProfile[] | null>(null)
const copied = ref(false)
const updateResult = ref<UpdateResult | null>(null)
const checkingUpdate = ref(false)
const dismissedUpdate = ref(false)
let copyTimer: ReturnType<typeof setTimeout> | undefined
const title = computed(() => page.value === 'dashboard' ? '连接总览' : page.value === 'import' ? '导入连接' : page.value === 'logs' ? '运行日志' : page.value === 'settings' ? '设置' : '服务器')
const pageEnglish = computed(() => ({ dashboard:'Overview',servers:'Nodes',import:'Import',logs:'Activity',settings:'Settings' })[page.value]??'Workspace')
const subtitle = computed(() => page.value === 'import' ? '把熟悉的连接，带到新的工作空间。' : page.value === 'logs' ? '查看连接与认证过程，定位软件运行问题。' : page.value === 'settings' ? '让你的本机工作空间更顺手。' : '集中管理连接，让每一次访问更简单。')
const groups = computed(() => [...new Set(state.profiles.map((profile) => profile.group).filter(Boolean))].sort().map((name) => ({ name, count: state.profiles.filter((profile) => profile.group === name).length })))
const sourceOptions = computed(() => [...new Set(state.profiles.map((profile) => profile.source))])
const filtered = computed(() => filterProfiles(state.profiles, query.value, group.value, source.value, auth.value))
const selected = computed(() => state.profiles.find((profile) => profile.id === selectedId.value))
const credentialCount = computed(() => state.profiles.filter(needsCredential).length)
const enabledCount = computed(() => state.profiles.filter((profile) => profile.enabled).length)
const busy = computed(() => !state.desktop || !state.ready || !!state.mutation)
const canTest = computed(() => !busy.value && state.engine.running && !state.engineBusy && !!selected.value?.enabled)
watch(filtered, (profiles, previous) => {
  if (!profiles.some(profile=>profile.id===selectedId.value)) {
    const index=Math.max(0,previous?.findIndex(profile=>profile.id===selectedId.value) ?? 0)
    selectedId.value=profiles[Math.min(index,profiles.length-1)]?.id ?? ''
  }
})
function resetFilters() { query.value = ''; group.value = ''; source.value = ''; auth.value = '' }
function openServer(id:string) { resetFilters(); page.value='servers'; selectedId.value=id }
async function save(profile: ConnectionProfile) { if (await dock.saveProfile(profile)) { editor.value = null; selectedId.value = profile.id } }
function confirmDelete(profiles: ConnectionProfile[]) {
  if (busy.value || !profiles.length) return
  state.notice = null
  deleting.value = profiles
}
function confirmBatch(ids: string[]) { confirmDelete(state.profiles.filter((profile) => ids.includes(profile.id))) }
async function remove() {
  if (!deleting.value?.length) return
  const profiles = deleting.value
  const success = profiles.length === 1 ? await dock.deleteProfile(profiles[0]!.id) : await dock.deleteProfiles(profiles.map((profile) => profile.id))
  if (success) deleting.value = null
}
async function copyEndpoint() {
  if (!state.desktop || !state.engine.running || !state.engine.endpoint) return
  try { await navigator.clipboard.writeText(state.engine.endpoint); copied.value = true; copyTimer = setTimeout(() => { copied.value = false }, 1800) }
  catch { dock.report('复制失败，请手动复制服务地址') }
}
async function checkUpdates() {
  if (!state.desktop || checkingUpdate.value) return
  checkingUpdate.value = true
  dismissedUpdate.value = false
  try {
    const currentVersion = await getVersion()
    updateResult.value = await checkLatestRelease(currentVersion, navigator.userAgent)
  } catch {
    updateResult.value = { status: 'unavailable', currentVersion: '', reason: '无法读取安装版本' }
  } finally { checkingUpdate.value = false }
}
async function openRelease() {
  if (updateResult.value?.status !== 'available') return
  try { await openUrl(RELEASE_PAGE) }
  catch { dock.report('无法打开发布页面，请检查系统默认浏览器') }
}
onMounted(async () => {
  if (!state.desktop) return
  await dock.initialize()
  if (state.ready) void checkUpdates()
})
onUnmounted(() => { dock.dispose(); if (copyTimer) clearTimeout(copyTimer) })
</script>
<template>
  <div class="app-shell">
    <Sidebar :page="page" :group="group" :groups="groups" :total="state.profiles.length" :engine="state.engine" :desktop="state.desktop" @navigate="page = $event" @group="group = $event" />
    <main class="workspace">
      <header class="topbar"><div class="breadcrumb"><span>星枢 Astralink</span><Icon name="chevron" :size="13" /><strong>{{ title }} <small class="breadcrumb-english">{{ pageEnglish }}</small></strong></div><div class="topbar-right"><ThemeSwitcher/><span class="local-label"><Icon name="monitor" :size="14" />本机工作空间</span><span class="user-avatar"><NexusMark :size="20"/></span></div></header>
      <div class="workspace-content">
        <section v-if="page !== 'dashboard'" class="page-header"><div><div class="eyebrow">ASTRALINK / {{ pageEnglish.toUpperCase() }}</div><h1>{{ title }}</h1><p>{{ subtitle }}</p></div><div v-if="page === 'servers'" class="button-row"><button class="button secondary" @click="page = 'import'"><Icon name="download" :size="16" />导入连接</button><button class="button primary" :disabled="busy" @click="editor = {}"><Icon name="plus" :size="16" />添加服务器</button></div></section>
        <div v-if="!state.desktop" class="desktop-banner" role="status"><span class="banner-icon"><Icon name="monitor" :size="20" /></span><div><strong>浏览器预览 · 需要桌面应用</strong><p>启动星枢 Astralink 桌面应用后，可读取本机配置、管理 MCP 和测试真实连接。</p></div><span class="badge blue">仅界面预览</span></div>
        <div v-if="state.loading" class="notice info" role="status"><span class="spinner" />正在加载本机工作空间…</div>
        <div v-if="state.desktop && !state.ready && !state.loading" class="notice error" role="alert"><Icon name="alert" :size="18" /><span>桌面初始化失败，操作暂不可用。</span><button class="text-button" @click="dock.initialize()">重试</button></div>
        <div v-if="state.notice" :class="['notice', state.notice.kind]" :role="state.notice.kind === 'error' ? 'alert' : 'status'"><Icon :name="state.notice.kind === 'success' ? 'check' : state.notice.kind === 'error' ? 'alert' : 'info'" :size="18" /><span>{{ state.notice.text }}</span><button class="icon-button" aria-label="关闭提示" @click="state.notice = null"><Icon name="close" :size="15" /></button></div>
        <div v-if="updateResult?.status === 'available' && !dismissedUpdate" class="notice info" role="status"><Icon name="download" :size="18" /><span>发现 Astralink {{ updateResult.latestVersion }}，当前为 {{ updateResult.currentVersion }}。</span><button class="button secondary" @click="openRelease">查看安装包</button><button class="icon-button" aria-label="暂不更新" @click="dismissedUpdate = true"><Icon name="close" :size="15" /></button></div>
        <DashboardPanel v-if="page === 'dashboard'" :dock="dock" @navigate="page=$event" @open-server="openServer" @create="editor={}"/>
        <template v-else-if="page === 'servers'">
          <div class="overview-grid">
            <section class="stat-card card"><span class="stat-icon blue"><Icon name="server" :size="21" /></span><div><span>服务器总数</span><strong>{{ state.profiles.length }}<small>台</small></strong></div><span class="stat-foot">{{ groups.length }} 个分组</span></section>
            <section class="stat-card card"><span class="stat-icon green"><Icon name="shield" :size="21" /></span><div><span>已启用连接</span><strong>{{ enabledCount }}<small>台</small></strong></div><span class="stat-foot">{{ credentialCount ? `${credentialCount} 台待补凭据` : '由本机统一管理' }}</span></section>
            <section class="mcp-card card"><span :class="['stat-icon', state.desktop && state.engine.running ? 'green' : 'slate']"><Icon name="terminal" :size="21" /></span><div class="mcp-copy"><span>MCP 服务 <span :class="['status-dot', { live: state.desktop && state.engine.running }]" /></span><strong>{{ !state.desktop ? '需要桌面应用' : state.engine.running ? '运行中' : '未启动' }}</strong><button v-if="state.desktop && state.engine.running && state.engine.endpoint" class="endpoint-copy mono" :title="state.engine.endpoint" @click="copyEndpoint">{{ state.engine.endpoint }}<Icon :name="copied ? 'check' : 'copy'" :size="12" /></button><small v-else>仅在本机提供安全访问</small></div><button class="button small" :class="state.engine.running ? 'secondary' : 'primary'" :disabled="busy || state.engineBusy" @click="dock.setEngine(!state.engine.running)"><span v-if="state.engineBusy" class="spinner" /><Icon v-else :name="state.engine.running ? 'stop' : 'play'" :size="14" />{{ state.engineBusy ? '处理中' : state.engine.running ? '停止' : '启动' }}</button></section>
          </div>
          <div v-if="state.engine.error" class="notice error"><Icon name="alert" :size="18" /><span>{{ state.engine.error }}</span><button class="text-button" :disabled="state.engineBusy" @click="dock.refreshEngine()">刷新状态</button></div>
          <div class="server-workspace"><section class="server-main"><div class="filter-bar"><label class="search-box"><Icon name="search" :size="18" /><input v-model="query" aria-label="搜索服务器" placeholder="搜索名称、地址或账号…" /><button v-if="query" class="icon-button" aria-label="清除搜索" @click="query = ''"><Icon name="close" :size="14" /></button></label><label><span class="sr-only">分组筛选</span><select v-model="group"><option value="">全部分组</option><option v-for="item in groups" :key="item.name" :value="item.name">{{ item.name }}</option></select></label><label><span class="sr-only">来源筛选</span><select v-model="source"><option value="">全部来源</option><option v-for="item in sourceOptions" :key="item" :value="item">{{ item === 'manual' ? '手动添加' : sourceLabels[item as SourceKind] || item }}</option></select></label><label><span class="sr-only">认证筛选</span><select v-model="auth"><option value="">全部认证</option><option v-for="(label, value) in authLabels" :key="value" :value="value">{{ label }}</option></select></label></div>
          <ServerTable :profiles="filtered" :selected-id="selectedId" :results="state.results" :testing="state.testing" :desktop="state.desktop" :ready="state.ready" :running="state.engine.running" :busy="!!state.mutation || state.engineBusy" :filtered="!!(query || group || source || auth)" :filter-key="JSON.stringify([query,group,source,auth])" @select="selectedId = $event" @edit="editor = { profile: $event }" @test="dock.testConnection" @delete="confirmDelete([$event])" @delete-batch="confirmBatch" @create="editor = {}" @import="page = 'import'" @reset="resetFilters" />
          <div class="codex-callout"><span class="codex-mark"><Icon name="link" :size="21" /></span><div><strong>让 AI 客户端连接你的服务器</strong><p>在客户端选项卡中一键接入 MCP。</p></div><button class="button secondary" @click="page = 'settings'">配置客户端<Icon name="arrow" :size="15" /></button></div></section>
          <ConnectionPanel :profile="selected" :result="selected ? state.results[selected.id] : undefined" :testing="!!(selected && state.testing[selected.id])" :auth-pending="!!selected && state.challenges.some((item) => item.profileId === selected!.id)" :can-test="canTest" :busy="busy" @edit="editor = { profile: $event }" @delete="confirmDelete([$event])" @test="dock.testConnection" /></div>
        </template>
        <ImportPanel v-else-if="page === 'import'" :dock="dock" />
        <RuntimeLogsPanel v-else-if="page === 'logs'" :dock="dock" />
        <SettingsPanel v-else :dock="dock" :update-result="updateResult" :checking-update="checkingUpdate" @check-update="checkUpdates" @open-release="openRelease" />
        <footer v-if="page !== 'dashboard'" class="workspace-footer"><span>星枢 · Astralink — 一处连接，万物可达。</span><span><Icon name="shield" :size="13" />本地配置 · 系统凭据库</span></footer>
      </div>
    </main>
    <HostEditor v-if="editor" :profile="editor.profile" :profiles="state.profiles" :busy="!!state.mutation" :error="state.notice?.kind === 'error' ? state.notice.text : undefined" :pick-file="() => dock.pickFile()" :load-secret="dock.revealProfileSecret" @save="save" @close="editor = null" />
    <Modal v-if="deleting" title="删除服务器" :busy="!!state.mutation" @close="deleting = null"><div class="delete-confirm"><span class="delete-icon"><Icon name="trash" :size="26" /></span><h3>{{ deleting.length === 1 ? '删除「' + deleting[0]!.name + '」？' : '删除选中的 ' + deleting.length + ' 台服务器？' }}</h3><p>删除后，这些连接将从星枢 Astralink 移除，后续 MCP 请求无法再使用。</p><ul class="delete-targets"><li v-for="profile in deleting" :key="profile.id"><strong>{{ profile.name }}</strong><span class="mono">{{ profile.username }}@{{ profile.host }}:{{ profile.port }}</span></li></ul><p v-if="state.notice?.kind === 'error'" class="field-error" role="alert">{{ state.notice.text }}</p></div><template #footer><button class="button secondary" :disabled="!!state.mutation" @click="deleting = null">取消</button><button class="button danger" :disabled="!!state.mutation" @click="remove"><span v-if="state.mutation" class="spinner" />确认删除</button></template></Modal>
    <OtpModal v-if="state.challenges[0]" :key="state.challenges[0].requestId" :challenge="state.challenges[0]" :queue-length="state.challenges.length" :busy="!!state.authBusy" :error="state.notice?.kind === 'error' ? state.notice.text : undefined" @submit="dock.submitOtp(state.challenges[0]!.requestId, $event)" @cancel="dock.cancelAuth(state.challenges[0]!.requestId)" />
  </div>
</template>
