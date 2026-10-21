<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { desktopBridge } from '../api'
import { clientGuides, copyAccessToken, createClientSnippet, mcpEndpoint } from '../clientConfigs'
import type { ClientId } from '../clientConfigs'
import type { AstralinkController } from '../controller'
import Icon from './Icon.vue'

const props = defineProps<{ dock: AstralinkController }>()
const { state } = props.dock
const selected = ref<ClientId>('claude-code')
const guide = computed(() => clientGuides.find((item) => item.id === selected.value)!)
const snippet = computed(() => createClientSnippet(selected.value, state.settings.mcpPort))
const endpoint = computed(() => mcpEndpoint(state.settings.mcpPort))
const registration = computed(() => state.clientRegistrations[selected.value])
const setupBusy = computed(() => !state.desktop || !state.ready || !state.engine.running || state.engineBusy || state.registrationBusy || !!state.mutation)
const busy = ref(false)
async function configure(pick = false) {
  if (setupBusy.value) return
  const client = selected.value
  let filePath: string | undefined
  if (pick || client === 'windsurf') {
    try { filePath = await props.dock.pickClientConfig() || undefined } catch { notify('无法打开配置文件选择器'); return }
    if (!filePath) return
  }
  await props.dock.registerClient(client, filePath)
}
const feedback = ref('')
let feedbackTimer: ReturnType<typeof setTimeout> | undefined

function notify(message: string) {
  feedback.value = message
  if (feedbackTimer) clearTimeout(feedbackTimer)
  feedbackTimer = setTimeout(() => { feedback.value = '' }, 5000)
}
async function copySnippet() {
  if (!snippet.value) return
  try {
    await navigator.clipboard.writeText(snippet.value)
    notify('配置片段已复制')
  } catch { notify('复制失败，请手动选择配置片段') }
}
async function copyToken() {
  if (!state.desktop || !state.ready || busy.value) return
  busy.value = true
  try {
    await copyAccessToken(
      () => desktopBridge.invoke('get_mcp_token', {}),
      (value) => navigator.clipboard.writeText(value),
    )
    notify('访问令牌已复制，请粘贴到目标客户端')
  } catch { notify('复制令牌失败，请检查桌面应用和剪贴板权限') }
  finally { busy.value = false }
}
onUnmounted(() => { if (feedbackTimer) clearTimeout(feedbackTimer) })
</script>

<template>
  <section class="client-setup" aria-labelledby="client-setup-title">
    <div class="client-setup-heading">
      <div><h3 id="client-setup-title">MCP 客户端接入</h3><p>选择客户端，按对应格式接入星枢 Astralink。</p></div>
      <span :class="['connection-indicator', { live: state.engine.running }]">
        <span class="status-dot" />{{ state.engine.running ? '服务运行中' : '服务未启动' }}
      </span>
    </div>
    <div class="endpoint-line"><span>本机 MCP 地址</span><code class="mono">{{ endpoint }}</code></div>
    <div class="client-tabs" role="tablist" aria-label="AI 客户端">
      <button v-for="item in clientGuides" :id="'client-tab-' + item.id" :key="item.id" type="button" role="tab"
        :aria-selected="selected === item.id" :tabindex="selected === item.id ? 0 : -1"
        aria-controls="client-config-panel" :class="{ selected: selected === item.id }" @click="selected = item.id">
        {{ item.name }}<span v-if="!item.supported" class="client-tab-limited" title="当前本机服务不能直接接入">待适配</span>
      </button>
    </div>
    <div id="client-config-panel" class="client-panel" role="tabpanel" :aria-labelledby="'client-tab-' + selected">
      <div class="client-panel-heading"><div><strong>{{ guide.name }}</strong><span class="client-location mono">{{ guide.location }}</span></div>
        <button v-if="snippet" type="button" class="button secondary" aria-label="复制配置片段" title="复制配置片段" @click="copySnippet"><Icon name="copy" :size="15" />复制配置</button>
      </div>
      <div v-if="guide.supported" class="client-quick-setup">
        <div class="client-quick-heading"><strong>快捷配置</strong><span>保留其他 MCP 条目，写入前自动备份。</span></div>
        <div class="button-row"><button type="button" class="button primary" :disabled="setupBusy" @click="configure()"><span v-if="state.registrationBusy" class="spinner" /><Icon v-else name="link" :size="15" />{{ selected === 'windsurf' ? '选择配置并接入' : '一键接入 ' + guide.name }}</button><button v-if="selected !== 'windsurf'" type="button" class="button secondary" :disabled="setupBusy" @click="configure(true)">选择其他配置文件</button></div>
        <p v-if="!state.engine.running">请先在服务器页面启动 MCP 服务。</p>
        <p>快捷配置写入用户级配置，自动填入访问令牌；也可选择你实际使用的配置文件。YAML 注释会移至备份；JSON 注释或 JSON5 配置请手动合并。</p>
        <div v-if="registration" class="registration-result" role="status"><strong><Icon name="check" :size="15" />{{ registration.reloadRequired ? '配置已更新' : '配置已是最新' }}</strong><p class="mono break-text">{{ registration.configPath }}</p><small v-if="registration.backupPath">备份：{{ registration.backupPath }}</small><p v-if="registration.reloadRequired">请重新加载 {{ guide.name }} 的 MCP 配置。</p></div>
      </div>
      <ol class="client-steps"><li v-for="step in guide.steps" :key="step">{{ step }}</li></ol>
      <div v-if="snippet" class="client-code-wrap"><pre class="client-code mono"><code>{{ snippet }}</code></pre></div>
      <div v-else class="client-unavailable"><Icon name="info" :size="18" /><span>当前不提供直接接入本机服务的配置。</span></div>
      <p v-if="guide.caution" class="client-caution"><Icon name="alert" :size="15" />{{ guide.caution }}</p>
      <div class="client-verify"><Icon name="check" :size="15" /><span>{{ guide.verify }}</span></div>
    </div>
    <div v-if="guide.supported" class="client-token-row"><span><strong>访问令牌</strong><small>片段仅含占位符。令牌不在页面显示；复制后请不要粘贴到公开文件或聊天中。</small></span>
      <button type="button" class="button secondary" :disabled="!state.desktop || !state.ready || busy" title="复制星枢 Astralink MCP 访问令牌" @click="copyToken"><Icon name="key" :size="15" />{{ busy ? '复制中…' : '复制令牌' }}</button>
    </div>
    <p v-if="feedback" class="client-feedback" role="status" aria-live="polite">{{ feedback }}</p>
  </section>
</template>

<style scoped>
.client-setup { grid-column: 1 / -1; min-width: 0; padding: 28px 0 8px; border-top: 1px solid #e1e7f0; }
.client-setup-heading { display: flex; justify-content: space-between; align-items: center; gap: 16px; margin-bottom: 20px; }
.client-setup-heading h3 { color: #253650; font-size: 16px; font-weight: 650; }
.client-setup-heading p { color: #718198; font-size: 12px; margin-top: 6px; }
.connection-indicator { display: inline-flex; align-items: center; gap: 7px; flex-shrink: 0; color: #718198; font-size: 11px; }
.connection-indicator.live { color: #178464; }
.endpoint-line { display: flex; align-items: center; gap: 14px; padding: 12px 0; border-top: 1px solid #edf1f6; color: #67768c; font-size: 11px; }
.endpoint-line code { color: #263b59; overflow-wrap: anywhere; user-select: all; }
.client-tabs { display: flex; gap: 4px; overflow-x: auto; border-bottom: 1px solid #dfe6ef; padding: 0 0 1px; scrollbar-width: thin; }
.client-tabs button { position: relative; flex: 0 0 auto; min-height: 38px; padding: 9px 11px; font-size: 11px; color: #607187; white-space: nowrap; }
.client-tabs button:hover { color: #2c5fc7; }
.client-tabs button.selected { color: #225dd7; font-weight: 650; }
.client-tabs button.selected::after { position: absolute; content: ''; left: 9px; right: 9px; bottom: -1px; height: 2px; background: #3269ed; }
.client-tab-limited { margin-left: 5px; font-size: 9px; color: #a0703e; }
.client-panel { min-width: 0; padding-top: 20px; }
.client-panel-heading { display: flex; justify-content: space-between; align-items: flex-start; gap: 12px; }
.client-panel-heading strong { display: block; color: #263850; font-size: 13px; }
.client-location { display: block; margin-top: 7px; color: #647793; font-size: 11px; overflow-wrap: anywhere; }
.client-panel-heading button { flex-shrink: 0; }
.client-steps { margin: 17px 0; padding-left: 20px; color: #4f6076; font-size: 12px; line-height: 1.8; }
.client-steps li { padding-left: 4px; }
.client-code-wrap { max-height: 340px; overflow: auto; border: 1px solid #e1e6ed; border-radius: 6px; background: #151d2b; }
.client-code { min-width: max-content; margin: 0; padding: 18px; color: #dbe7f7; font-size: 11px; line-height: 1.7; white-space: pre; tab-size: 2; }
.client-unavailable { display: flex; align-items: center; gap: 9px; min-height: 90px; padding: 15px; color: #7f5f33; background: #fff8e9; border-left: 3px solid #d6a355; font-size: 12px; }
.client-caution, .client-verify { display: flex; align-items: flex-start; gap: 8px; padding-top: 13px; color: #7b6b55; font-size: 11px; line-height: 1.6; }
.client-verify { color: #456d60; }
.client-token-row { display: flex; justify-content: space-between; align-items: center; gap: 12px; border-top: 1px solid #e6eaf0; margin-top: 22px; padding-top: 18px; }
.client-token-row strong, .client-token-row small { display: block; }
.client-token-row strong { font-size: 12px; color: #2b405b; }
.client-token-row small { font-size: 11px; color: #8492a5; margin-top: 5px; line-height: 1.5; }
.client-token-row button { flex-shrink: 0; }
.client-feedback { margin-top: 12px; color: #3269ed; font-size: 11px; }
@media (max-width: 720px) {
  .client-setup { padding-top: 18px; }
  .client-setup-heading, .client-token-row { align-items: flex-start; flex-direction: column; }
  .client-steps { font-size: 11px; }
  .client-panel-heading { align-items: flex-start; }
  .client-panel-heading button { padding: 8px; }
}
</style>

<style scoped>
.client-quick-setup { margin-top: 18px; padding: 16px; background: #f5f8fd; border: 1px solid #e1e7f0; border-radius: 8px; }
.client-quick-heading { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; margin-bottom: 12px; font-size: 12px; }
.client-quick-heading span, .client-quick-setup > p { color: #718198; font-size: 11px; line-height: 1.7; }
.client-quick-setup > p { margin-top: 9px; }
.client-quick-setup .button-row { flex-wrap: wrap; }
</style>
