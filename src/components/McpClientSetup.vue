<script setup lang="ts">
import { copyText } from '../clipboard'
import { computed, onUnmounted, ref, watch } from 'vue'
import { activeBridge, desktopBridge } from '../api'
import { webMode, currentWebUser } from '../webBridge'
import { clientGuides, copyAccessToken, createClientSnippet, mcpEndpoint } from '../clientConfigs'
import type { ClientId } from '../clientConfigs'
import type { AstraLinkController } from '../controller'
import Icon from './Icon.vue'
import Modal from './Modal.vue'

const props = defineProps<{ dock: AstraLinkController }>()
const { state } = props.dock
const selected = ref<ClientId>(webMode ? 'codex' : 'claude-code')
const guide = computed(() => clientGuides.find((item) => item.id === selected.value)!)
const webConnection = ref<'same' | 'tunnel'>('same')
const forwardPort = ref(18767)
const sshUser = ref('')
const sshHost = ref('')
const localPort = computed(() => webMode && webConnection.value === 'tunnel' ? forwardPort.value : state.settings.mcpPort)
const clientHost = computed(() => webMode && webConnection.value === 'same' ? state.mcpClientHost || '127.0.0.1' : '127.0.0.1')
const webHttpClient = computed(() => selected.value !== 'chatgpt')
const tunnelCommand = computed(() => `ssh -N -L 127.0.0.1:${forwardPort.value}:127.0.0.1:${state.settings.mcpPort} ${sshUser.value}@${sshHost.value}`)
const tunnelValid = computed(() => Number.isInteger(forwardPort.value) && forwardPort.value >= 1024 && forwardPort.value <= 65535 && /^[a-zA-Z0-9._-]+$/.test(sshUser.value) && /^[a-zA-Z0-9._-]+$/.test(sshHost.value))
const bridgePaths = ref<{ nodePath: string; scriptPath: string }>()
watch(() => state.desktop && state.ready, async (ready) => {
  if (!ready || webMode) { bridgePaths.value = undefined; return }
  try { bridgePaths.value = await desktopBridge.invoke('get_mcp_bridge_paths', {}) }
  catch { bridgePaths.value = undefined }
}, { immediate: true })
const snippet = computed(() => webMode && (!webHttpClient.value || !Number.isInteger(localPort.value) || localPort.value < 1024 || localPort.value > 65535) ? null : createClientSnippet(selected.value, localPort.value, bridgePaths.value, clientHost.value, webMode ? 'server' : 'desktop'))
const endpoint = computed(() => Number.isInteger(localPort.value) && localPort.value >= 1024 && localPort.value <= 65535 ? mcpEndpoint(localPort.value, clientHost.value) : '请输入有效的客户端端口')
const registration = computed(() => state.clientRegistrations[selected.value])
const setupBusy = computed(() => !state.desktop || !state.ready || !state.engine.running || state.engineBusy || state.registrationBusy || !!state.mutation)
const busy = ref(false)
const tokenDialog = ref(false)
const personalToken = ref('')
const tokenError = ref('')
async function configure(pick = false) {
  if (webMode || setupBusy.value) return
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
    await copyText(snippet.value)
    notify('配置片段已复制')
  } catch { notify('复制失败，请手动选择配置片段') }
}
async function copyToken() {
  if (!state.desktop || !state.ready || busy.value) return
  if (webMode) { tokenError.value = ''; personalToken.value = ''; tokenDialog.value = true; return }
  busy.value = true
  try {
    await copyAccessToken(
      () => activeBridge.invoke('get_mcp_token', {}),
      (value) => copyText(value),
    )
    notify('访问令牌已复制，请粘贴到目标客户端')
  } catch { notify('复制令牌失败，请检查服务状态和剪贴板权限') }
  finally { busy.value = false }
}
function closeTokenDialog() { if (!busy.value) { personalToken.value = ''; tokenDialog.value = false } }
async function generateToken() {
  if (busy.value || personalToken.value) return
  busy.value = true; tokenError.value = ''
  try { personalToken.value = await activeBridge.invoke('get_mcp_token', {}) }
  catch { tokenError.value = '签发失败，请检查当前账号和服务状态' }
  finally { busy.value = false }
}
async function copyPersonalToken() {
  tokenError.value = ''
  try { await copyText(personalToken.value); notify('个人令牌已复制') }
  catch { tokenError.value = '复制失败，请选中令牌手动复制' }
}
async function copyTunnel() {
  if (!tunnelValid.value) return
  try { await copyText(tunnelCommand.value); notify('SSH 隧道命令已复制') }
  catch { notify('复制失败，请手动选择命令') }
}
onUnmounted(() => { if (feedbackTimer) clearTimeout(feedbackTimer) })
</script>

<template>
  <section class="client-setup" aria-labelledby="client-setup-title">
    <div class="client-setup-heading">
      <div><h3 id="client-setup-title">MCP 客户端接入</h3><p>{{ webMode ? '选择客户端，配置同机访问或 SSH 隧道。' : '选择客户端，按对应格式接入AstraLink。' }}</p></div>
      <span :class="['connection-indicator', { live: state.engine.running }]">
        <span class="status-dot" />{{ state.engine.running ? '服务运行中' : '服务未启动' }}
      </span>
    </div>
    <div v-if="webMode" class="web-connection">
      <div class="connection-modes" role="group" aria-label="客户端位置"><button type="button" :aria-pressed="webConnection === 'same'" :class="{ active: webConnection === 'same' }" @click="webConnection = 'same'">{{ state.mcpClientHost === '127.0.0.1' ? '与服务同机' : '指定地址' }}</button><button type="button" :aria-pressed="webConnection === 'tunnel'" :class="{ active: webConnection === 'tunnel' }" @click="webConnection = 'tunnel'">远程 SSH 隧道</button></div>
      <div v-if="webConnection === 'tunnel'" class="tunnel-fields"><label class="field"><span>SSH 用户名</span><input v-model.trim="sshUser" autocomplete="off" placeholder="ops" /></label><label class="field"><span>SSH 主机</span><input v-model.trim="sshHost" autocomplete="off" placeholder="server.example.com" /></label><label class="field"><span>客户端本地端口</span><input v-model.number="forwardPort" type="number" min="1024" max="65535" /></label></div>
      <div v-if="webConnection === 'tunnel'" class="tunnel-command"><code class="mono">{{ tunnelValid ? tunnelCommand : '填写 SSH 账号、主机和客户端本地端口' }}</code><button type="button" class="icon-button" title="复制 SSH 隧道命令" aria-label="复制 SSH 隧道命令" :disabled="!tunnelValid" @click="copyTunnel"><Icon name="copy" :size="16" /></button></div>
      <p class="client-caution">远程直连需在容器设置 ASTRALINK_MCP_REMOTE_HOST 为服务器 IP，并映射 MCP 端口；仅修改展示 IP 不会开放端口。Docker 部署的 SSH 隧道也需要先把 MCP 端口映射到宿主机回环地址。</p>
    </div>
    <div class="endpoint-line"><span>{{ webMode ? '客户端使用的 MCP 地址' : '本机 MCP 地址' }}</span><code class="mono">{{ endpoint }}</code></div>
    <div class="client-tabs" role="tablist" aria-label="AI 客户端">
      <button v-for="item in clientGuides" :id="'client-tab-' + item.id" :key="item.id" type="button" role="tab"
        :aria-selected="selected === item.id" :tabindex="selected === item.id ? 0 : -1"
        aria-controls="client-config-panel" :class="{ selected: selected === item.id }" @click="selected = item.id">
        {{ item.name }}<span v-if="webMode && item.id === 'claude-desktop'" class="client-tab-limited">需 Node 桥接</span><span v-else-if="!item.supported" class="client-tab-limited" title="需要公网 HTTPS 网关">需远程网关</span>
      </button>
    </div>
    <div id="client-config-panel" class="client-panel" role="tabpanel" :aria-labelledby="'client-tab-' + selected">
      <div class="client-panel-heading"><div><strong>{{ guide.name }}</strong><span class="client-location mono">{{ guide.location }}</span></div>
        <button v-if="snippet" type="button" class="button secondary" aria-label="复制配置片段" title="复制配置片段" @click="copySnippet"><Icon name="copy" :size="15" />复制配置</button>
      </div>
      <div v-if="!webMode && guide.supported && guide.quickSetup !== false" class="client-quick-setup">
        <div class="client-quick-heading"><strong>快捷配置</strong><span>保留其他 MCP 条目，写入前自动备份。</span></div>
        <div class="button-row"><button type="button" class="button primary" :disabled="setupBusy" @click="configure()"><span v-if="state.registrationBusy" class="spinner" /><Icon v-else name="link" :size="15" />{{ selected === 'windsurf' ? '选择配置并接入' : '一键接入 ' + guide.name }}</button><button v-if="selected !== 'windsurf'" type="button" class="button secondary" :disabled="setupBusy" @click="configure(true)">选择其他配置文件</button></div>
        <p v-if="!state.engine.running">请先在服务器页面启动 MCP 服务。</p>
        <p>快捷配置写入用户级配置，自动填入访问令牌；也可选择你实际使用的配置文件。YAML 注释会移至备份；JSON 注释或 JSON5 配置请手动合并。</p>
        <div v-if="registration" class="registration-result" role="status"><strong><Icon name="check" :size="15" />{{ registration.reloadRequired ? '配置已更新' : '配置已是最新' }}</strong><p class="mono break-text">{{ registration.configPath }}</p><small v-if="registration.backupPath">备份：{{ registration.backupPath }}</small><p v-if="registration.reloadRequired">请重新加载 {{ guide.name }} 的 MCP 配置。</p></div>
      </div>
      <ol v-if="!webMode" class="client-steps"><li v-for="step in guide.steps" :key="step">{{ step }}</li></ol>
      <ol v-else-if="webHttpClient" class="client-steps"><li>{{ webConnection === 'tunnel' ? '先在运行 MCP 客户端的电脑上建立上方 SSH 隧道。' : clientHost === '127.0.0.1' ? '在运行 AstraLink 的服务器上启动 MCP 客户端。' : '先确认指定 IP 上的受保护 MCP 转发入口可从客户端访问。' }}</li><li>将下方片段并入该客户端配置，再单独填写个人 MCP 访问令牌。WorkBuddy、Cherry Studio 的 HTTP 接入不需要安装 AstraLink 桌面版。</li></ol>
      <p v-if="webMode && selected === 'claude-desktop'" class="client-caution">此片段在运行 Claude Desktop 的电脑上使用 Node.js / npx 和 mcp-remote 桥接，不需要安装 AstraLink 桌面版。首次使用会下载桥接包。</p>
      <p v-if="webMode && selected === 'cherry-studio'" class="client-caution">在 Cherry Studio 的 MCP 设置中选择 Streamable HTTP，填写上述 URL，并添加 Authorization 请求头。若版本不接受 JSON 导入字段，请按界面手动添加。</p>
      <p v-if="webMode && selected === 'chatgpt'" class="client-caution">云端客户端不能直接使用服务器回环地址；需要单独部署受保护的 HTTPS MCP 网关。</p>
      <div v-if="snippet" class="client-code-wrap"><pre class="client-code mono"><code>{{ snippet }}</code></pre></div>
      <div v-else-if="!webMode" class="client-unavailable"><Icon name="info" :size="18" /><span>本机服务不能直接连接云端 ChatGPT。</span></div>
      <p v-if="!webMode && guide.caution" class="client-caution"><Icon name="alert" :size="15" />{{ guide.caution }}</p>
      <div v-if="!webMode || webHttpClient" class="client-verify"><Icon name="check" :size="15" /><span>{{ guide.verify }}</span></div>
    </div>
    <div v-if="guide.supported && (!webMode || webHttpClient)" class="client-token-row"><span><strong>访问令牌</strong><small>片段仅含占位符。令牌不在页面显示；复制后请不要粘贴到公开文件或聊天中。</small></span>
      <button type="button" class="button secondary" :disabled="!state.desktop || !state.ready || busy || (webMode && currentWebUser?.role === 'viewer')" title="生成个人 MCP 访问令牌" @click="copyToken"><Icon name="key" :size="15" />{{ busy ? '生成中…' : webMode ? '生成个人令牌' : '复制令牌' }}</button>
    </div>
    <Modal v-if="tokenDialog" title="个人 MCP 令牌" :busy="busy" @close="closeTokenDialog">
      <p class="client-caution">{{ personalToken ? '令牌仅显示这一次。关闭前请复制保存。' : '生成新令牌后，旧令牌将立即失效，请更新客户端配置。' }}</p>
      <label v-if="personalToken" class="field"><span>访问令牌</span><input :value="personalToken" readonly autocomplete="off" @focus="($event.target as HTMLInputElement).select()" /></label>
      <p v-if="tokenError" class="field-error" role="alert">{{ tokenError }}</p>
      <template #footer><button class="button secondary" :disabled="busy" @click="closeTokenDialog">{{ personalToken ? '完成' : '取消' }}</button><button class="button primary" :disabled="busy" @click="personalToken ? copyPersonalToken() : generateToken()">{{ busy ? '生成中…' : personalToken ? '复制令牌' : '确认生成' }}</button></template>
    </Modal>
    <p v-if="feedback" class="client-feedback" role="status" aria-live="polite">{{ feedback }}</p>
  </section>
</template>

<style scoped>
.client-setup { grid-column: 1 / -1; min-width: 0; padding: 28px 0 8px; border-top: 1px solid #e1e7f0; }
.web-connection { border-top: 1px solid var(--border, #dfe6ef); padding: 14px 0; }
.connection-modes { display: inline-flex; border: 1px solid var(--border, #dfe6ef); border-radius: 6px; padding: 3px; gap: 2px; }
.connection-modes button { padding: 7px 12px; border-radius: 4px; font-size: 11px; color: var(--text-muted, #66788b); }
.connection-modes button.active { background: var(--surface-muted, #eef3f7); color: var(--text, #243b52); font-weight: 650; }
.tunnel-fields { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin: 12px 0; }
.tunnel-command { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-width: 0; padding: 10px; border: 1px solid var(--border, #dfe6ef); border-radius: 6px; }
.tunnel-command code { overflow-wrap: anywhere; font-size: 11px; }
@media (max-width: 720px) { .tunnel-fields { grid-template-columns: 1fr; } }
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
