<script setup lang="ts">
import { copyText } from '../clipboard'
import { computed, onMounted, reactive, ref, watch } from 'vue'
import type { AstraLinkController } from '../controller'
import type { UpdateResult } from '../updateCheck'
import { version as appVersion } from '../../package.json'
import { validateSettings } from '../domain'
import Icon from './Icon.vue'
import McpClientSetup from './McpClientSetup.vue'
import CloudBackupPanel from './CloudBackupPanel.vue'
import AppearancePanel from './AppearancePanel.vue'
import NexusMark from './NexusMark.vue'
import { webMode, changeWebPassword, currentWebUser } from '../webBridge'
const props = defineProps<{ dock: AstraLinkController; updateResult: UpdateResult | null; checkingUpdate: boolean }>()
const emit = defineEmits<{ 'check-update': []; 'open-release': [] }>()
const { state } = props.dock
type SettingsTab = 'mcp' | 'appearance' | 'data' | 'security'
const activeTab = ref<SettingsTab>('mcp')
const tabs: { id: SettingsTab; label: string }[] = [
  { id: 'mcp', label: 'MCP 接入' },
  { id: 'appearance', label: '外观' },
  { id: 'data', label: '数据与备份' },
  { id: 'security', label: '安全与更新' },
]
const form = reactive({ ...state.settings })
const error = computed(() => validateSettings(form))
const changed = computed(() => JSON.stringify(form) !== JSON.stringify(state.settings))
watch(() => state.settings, (settings) => Object.assign(form, settings))
const storageMessage = ref('')
const passwords = reactive({ current: '', next: '' })
const passwordMessage = ref('')
const passwordBusy = ref(false)
async function updateAdminPassword() {
  passwordBusy.value = true; passwordMessage.value = ''
  try {
    await changeWebPassword(passwords.current, passwords.next)
    passwords.current = ''; passwords.next = ''
    window.location.reload()
  } catch (error) { passwordMessage.value = error instanceof Error ? error.message : '修改失败' }
  finally { passwordBusy.value = false }
}
async function copyDatabasePath() {
  if (!state.storageInfo) return
  try { await copyText(state.storageInfo.databasePath); storageMessage.value='数据库路径已复制' }
  catch { storageMessage.value='复制失败，请手动复制路径' }
}
onMounted(() => { if(state.desktop && state.ready) void props.dock.refreshStorageInfo() })
watch(() => state.ready, ready => { if(ready && state.desktop) void props.dock.refreshStorageInfo() })
</script>
<template>
  <div class="settings-layout">
    <div class="settings-tabs" role="tablist" aria-label="设置分类">
      <button v-for="tab in tabs" :key="tab.id" type="button" role="tab" :aria-selected="activeTab === tab.id" :class="{ active: activeTab === tab.id }" @click="activeTab = tab.id">{{ tab.label }}</button>
    </div>
    <AppearancePanel v-if="activeTab === 'appearance'" />
    <section v-if="activeTab === 'mcp'" class="card settings-card settings-full"><div class="section-heading"><span class="section-icon"><Icon name="settings" :size="21" /></span><div><h3>MCP 服务</h3><p>{{ webMode ? '管理服务器内部的 MCP 引擎。' : '管理本机 MCP 与桌面行为。' }}</p></div></div>
      <form @submit.prevent="dock.saveSettings({ ...form })"><fieldset :disabled="!state.desktop || !state.ready || !!state.mutation || (webMode && currentWebUser?.role !== 'admin')"><legend class="sr-only">应用偏好</legend>
        <div class="setting-row"><div><h4>MCP 服务端口</h4><p>端口变更后重启服务生效。</p></div><label class="field port-field"><span class="sr-only">MCP 服务端口</span><input v-model.number="form.mcpPort" type="number" min="1024" max="65535" step="1" :aria-invalid="!!error" /></label></div><p v-if="error" class="field-error">{{ error }}</p>
        <label class="setting-row"><span><strong>启动时开启 MCP</strong><small>{{ webMode ? '服务器启动时自动开启服务。' : '打开AstraLink后自动启动服务。' }}</small></span><input v-model="form.startOnLaunch" type="checkbox" role="switch" class="switch" /></label>
        <label v-if="!webMode" class="setting-row"><span><strong>关闭窗口时最小化到托盘</strong><small>保持 MCP 服务和进行中的连接。</small></span><input v-model="form.minimizeToTray" type="checkbox" role="switch" class="switch" /></label>
      </fieldset><div class="settings-footer"><span>{{ changed ? '有未保存的更改' : '设置已同步' }}</span><button v-if="!webMode || currentWebUser?.role === 'admin'" class="button primary" type="submit" :disabled="!state.desktop || !state.ready || !!state.mutation || !changed || !!error"><span v-if="state.mutation === 'settings'" class="spinner" />保存设置</button></div></form>
    </section>
    <McpClientSetup v-if="activeTab === 'mcp'" :dock="dock" />
    <CloudBackupPanel v-if="activeTab === 'data' && (!webMode || currentWebUser?.role === 'admin')" :dock="dock" />
    <section v-if="activeTab === 'data'" class="card settings-card storage-card"><div class="section-heading"><span class="section-icon"><Icon name="server" :size="21" /></span><div><h3>{{ webMode ? '服务器数据' : '本机数据' }}</h3><p>服务器配置和应用设置保存在 SQLite 数据库。</p></div></div><template v-if="state.storageInfo"><dl class="storage-location"><div><dt>数据库文件</dt><dd><code>{{ state.storageInfo.databasePath }}</code></dd></div><div v-if="state.storageInfo.legacyBackupPath"><dt>旧配置备份</dt><dd><code>{{ state.storageInfo.legacyBackupPath }}</code></dd></div><div><dt>服务器数量</dt><dd>{{ state.storageInfo.serverCount }} 台</dd></div></dl><p class="storage-note">{{ webMode ? '连接凭据加密保存在服务器数据目录。请同时备份数据目录和主密钥。' : '密码、私钥口令和动态密钥单独保存在系统凭据库。旧 JSON 仅作为迁移备份，不再更新。' }}</p><button class="button secondary" @click="copyDatabasePath"><Icon name="copy" :size="15" />复制数据库路径</button><small v-if="storageMessage" role="status">{{ storageMessage }}</small></template><p v-else class="storage-note">{{ !state.desktop ? '请在桌面应用中查看存储位置' : state.storageInfoBusy ? '正在读取存储位置…' : '存储信息尚未读取' }}</p><button v-if="state.desktop" class="text-button" :disabled="!state.ready || state.storageInfoBusy" @click="dock.refreshStorageInfo()">刷新存储信息</button></section>
    <section v-if="activeTab === 'security' && webMode" class="card settings-card settings-full"><div class="section-heading"><span class="section-icon"><Icon name="shield" :size="21" /></span><div><h3>我的密码</h3></div></div><form class="security-form" @submit.prevent="updateAdminPassword"><label class="field"><span>当前密码</span><input v-model="passwords.current" type="password" autocomplete="current-password" required /></label><label class="field"><span>新密码</span><input v-model="passwords.next" type="password" autocomplete="new-password" minlength="12" required /></label><button class="button primary" type="submit" :disabled="passwordBusy || passwords.next.length < 12">修改密码</button><p v-if="passwordMessage" class="field-error" role="alert">{{ passwordMessage }}</p></form></section>
    <section v-if="activeTab === 'security' && !webMode" class="card settings-card settings-full"><div class="section-heading"><span class="section-icon"><Icon name="download" :size="21" /></span><div><h3>软件更新</h3><p>启动后自动检查 GitHub 稳定版本。</p></div></div><div class="setting-row"><span><strong v-if="checkingUpdate">正在检查更新…</strong><strong v-else-if="updateResult?.status === 'available'">发现新版本 {{ updateResult.latestVersion }}</strong><strong v-else-if="updateResult?.status === 'current'">已是最新版本</strong><strong v-else-if="updateResult?.status === 'unavailable'">{{ updateResult.reason }}</strong><strong v-else>尚未检查</strong><small v-if="updateResult?.currentVersion">当前版本 {{ updateResult.currentVersion }}</small></span><div class="button-row"><button type="button" class="button secondary" :disabled="!state.desktop || !state.ready || checkingUpdate" @click="emit('check-update')">检查更新</button><button v-if="updateResult?.status === 'available'" type="button" class="button primary" @click="emit('open-release')">查看安装包</button></div></div></section>
    <section class="settings-about"><span class="brand-mark"><NexusMark :size="29"/></span><div><strong>AstraLink</strong><p>v{{ appVersion }} · 一处连接，万物可达。</p></div><span><Icon name="shield" :size="16" />{{ webMode ? '连接凭据由服务器加密保存' : '密码与私钥口令由系统凭据库管理' }}</span></section>
  </div>
</template>
<style scoped>
.settings-tabs { grid-column: 1 / -1; display: flex; gap: 4px; border-bottom: 1px solid var(--border, #dfe6ef); overflow-x: auto; }
.settings-tabs button { white-space: nowrap; padding: 12px 16px; border-bottom: 2px solid transparent; color: var(--text-muted, #66788b); font-size: 12px; }
.settings-tabs button.active { border-bottom-color: #24948b; color: var(--text, #243b52); font-weight: 650; }
.settings-full { grid-column: 1 / -1; }
.security-form { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; align-items: end; }
.security-form button { justify-self: start; }
@media (max-width: 720px) { .settings-tabs button { padding: 11px; font-size: 11px; } .security-form { grid-template-columns: 1fr; } }
</style>
