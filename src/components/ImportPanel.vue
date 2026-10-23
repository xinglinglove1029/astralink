<script setup lang="ts">
import { webMode } from '../webBridge'
import { computed, onUnmounted, ref, watch } from 'vue'
import type { AstraLinkController } from '../controller'
import { inspectImport, needsCredential, sourceLabels } from '../domain'
import type { SourceKind } from '../types'
import Icon from './Icon.vue'
const props = defineProps<{ dock: AstraLinkController }>()
const { state } = props.dock
const kind = ref<SourceKind>('xshell')
const filePath = ref('')
const strategy = ref<'merge' | 'replace'>('merge')
const selected = ref<string[]>([])
const acknowledge = ref(false)
const picking = ref(false)
const localError = ref('')
const importPassword = ref('')
let active = true
let pickRevision = 0
const sources: { kind: SourceKind; note: string; icon: string }[] = [
  { kind: 'xshell', note: '目录 / .xsh / .xts', icon: 'terminal' },
  { kind: 'finalshell', note: 'conn 目录 / JSON', icon: 'terminal' },
  { kind: 'nyaterm', note: '本机 .redb / v1 JSON', icon: 'terminal' },
  { kind: 'mobaxterm', note: '.mxtsessions', icon: 'terminal' },
  { kind: 'windterm', note: 'user.sessions', icon: 'terminal' },
  { kind: 'securecrt', note: '导出 .xml', icon: 'terminal' },
  { kind: 'electerm', note: '书签 .json', icon: 'terminal' },
  { kind: 'termius', note: '本机 IndexedDB', icon: 'terminal' },
  { kind: 'nya-backup', note: '.nya 加密备份', icon: 'terminal' },
  { kind: 'tabby', note: 'config.yaml', icon: 'terminal' },
  { kind: 'netcatty', note: 'CSV 导出文件', icon: 'server' },
  { kind: 'mcp-json', note: '已有 MCP JSON 配置', icon: 'link' },
  { kind: 'openssh', note: 'SSH config 文件', icon: 'key' },
]
const picked = computed(() => state.preview?.profiles.filter((profile) => selected.value.includes(profile.id)) ?? [])
const inspection = computed(() => inspectImport(picked.value, strategy.value === 'replace' ? [] : state.profiles))
const conflicts = computed(() => inspectImport(picked.value, state.profiles).conflicts)
const risky = computed(() => inspection.value.needsCredential.length > 0 || conflicts.value.length > 0 || strategy.value === 'replace' || !!state.preview?.issues.length)
const canImport = computed(() => state.desktop && state.ready && !state.mutation && !state.previewBusy && picked.value.length > 0 && !inspection.value.errors.length && !state.preview?.issues.some((issue) => issue.severity === 'error') && (!risky.value || acknowledge.value))
watch(() => state.preview, (preview) => { selected.value = preview?.profiles.map((profile) => profile.id) ?? []; acknowledge.value = false })
watch([selected, strategy], () => { acknowledge.value = false }, { deep: true })
function changeSource(source: SourceKind) {
  kind.value = source
  filePath.value = ''
  localError.value = ''
  importPassword.value = ''
  pickRevision++
  props.dock.clearPreview()
}
async function chooseFile(selection: 'file' | 'directory' = 'file') {
  const revision = ++pickRevision
  const selectedKind = kind.value
  picking.value = true
  localError.value = ''
  try {
    const file = await props.dock.pickFile(selectedKind, selectedKind === 'termius' ? 'directory' : selection)
    if (active && revision === pickRevision && file) { filePath.value = file; await previewSource(selectedKind, file) }
  } catch { if (active) localError.value = '无法选择文件，请检查桌面文件权限' }
  finally { if (active) picking.value = false }
}
async function previewSource(sourceKind: SourceKind, path: string) {
  const secret = importPassword.value
  const result = await props.dock.previewImport(sourceKind, path, secret)
  if (result) importPassword.value = ''
}
function useDetected(sourceKind: string, path: string) { changeSource(sourceKind as SourceKind); filePath.value = path; void previewSource(kind.value, path) }
function pathChanged() { pickRevision++; props.dock.clearPreview(); acknowledge.value = false }
function toggleAll(event: Event) { selected.value = (event.target as HTMLInputElement).checked ? state.preview?.profiles.map((profile) => profile.id) ?? [] : [] }
onUnmounted(() => { active = false; pickRevision++; props.dock.clearPreview() })
</script>
<template>
  <div class="import-layout">
    <section class="card import-main">
      <div class="section-heading"><span class="step-number">1</span><div><h3>选择连接来源</h3><p>导入配置后，在AstraLink 统一维护。</p></div></div>
      <div class="source-grid"><button v-for="source in sources" :key="source.kind" :class="['source-card', { active: kind === source.kind }]" :disabled="!!state.mutation || picking" @click="changeSource(source.kind)"><span class="source-logo"><Icon :name="source.icon" :size="22" /></span><strong>{{ sourceLabels[source.kind] }}</strong><small>{{ source.note }}</small><Icon v-if="kind === source.kind" name="check" :size="15" class="source-check" /></button></div>
      <div class="file-drop"><span class="file-drop-icon"><Icon name="folder" :size="25" /></span><div><h4>选择 {{ sourceLabels[kind] }} 连接配置</h4><p>{{ webMode ? '选择配置文件上传并预览；目录型来源请先导出为单个文件。' : kind === 'netcatty' ? '请先从 Netcatty 导出 CSV 文件。' : kind === 'xshell' ? '选择会话目录批量导入，或选择 .xsh 文件 / .xts 导出包。' : kind === 'finalshell' ? '选择 FinalShell 的 conn 目录；加密密码需重新填写。' : kind === 'nyaterm' ? '选择本机 .redb 或 v1 JSON 配置；数据库凭据需补齐。' : kind === 'windterm' ? '选择 user.sessions；启用主密码时需输入后预览。' : kind === 'termius' ? '选择本机 IndexedDB 目录；读取当前系统凭据库中的 localKey。' : kind === 'nya-backup' ? '选择 .nya 加密备份并输入备份密码，仅提取 SSH 连接。' : '通过本机文件选择器读取，不上传到云端。' }}</p></div><div class="file-picker-actions"><button v-if="!webMode && (kind === 'xshell' || kind === 'finalshell')" class="button secondary" :disabled="!state.desktop || !state.ready || !!state.mutation || picking" @click="chooseFile('directory')"><Icon name="folder" :size="14" />选择目录</button><button class="button secondary" :disabled="!state.desktop || !state.ready || !!state.mutation || picking || (webMode && kind === 'termius')" @click="chooseFile('file')"><span v-if="picking" class="spinner" />{{ picking ? '选择中…' : kind === 'termius' ? '选择目录' : '选择文件' }}</button></div></div>
      <div class="path-row"><label class="sr-only" for="import-path">导入文件路径</label><input id="import-path" v-model="filePath" class="mono" :placeholder="webMode ? '请选择文件' : '或输入本机配置路径'" :readonly="webMode" :disabled="!state.desktop || !state.ready || !!state.mutation || picking" @input="pathChanged" /><button class="button secondary" :disabled="!filePath.trim() || !state.desktop || !state.ready || state.previewBusy || !!state.mutation || picking" @click="previewSource(kind, filePath)"><span v-if="state.previewBusy" class="spinner" />预览</button></div>
      <label v-if="kind === 'windterm' || kind === 'nya-backup'" class="field"><span>{{ kind === 'windterm' ? 'WindTerm 主密码（如已启用）' : 'NyaTerm 备份密码' }}</span><input v-model="importPassword" type="password" autocomplete="off" :disabled="!state.desktop || !state.ready || state.previewBusy || !!state.mutation" /><small>仅用于本次预览，不保存密码。</small></label>
      <p v-if="localError" class="field-error" role="alert">{{ localError }}</p>
      <div class="import-divider" />
      <div class="section-heading"><span class="step-number">2</span><div><h3>检查并导入</h3><p>确认服务器、冲突与认证信息。</p></div><span v-if="state.preview" class="badge blue">{{ state.preview.stats.total }} 个连接</span></div>
      <div v-if="state.previewBusy" class="preview-loading" role="status"><span class="spinner" />正在读取配置…</div>
      <template v-else-if="state.preview">
        <div class="import-stats"><span><strong>{{ state.preview.stats.ready }}</strong>可导入</span><span><strong>{{ state.preview.stats.needsCredential }}</strong>待补凭据</span><span><strong>{{ state.preview.stats.unsupported }}</strong>不支持</span><span><strong>{{ conflicts.length }}</strong>已有冲突</span></div>
        <div v-if="state.preview.issues.length || inspection.errors.length" class="import-warnings" role="status"><p v-for="(issue, index) in state.preview.issues" :key="index" :class="{ 'danger-text': issue.severity === 'error' }"><Icon :name="issue.severity === 'error' ? 'alert' : 'info'" :size="15" /><span>{{ issue.profileName ? `${issue.profileName}：` : '' }}{{ issue.message }}</span></p><p v-for="error in inspection.errors" :key="error" class="danger-text"><Icon name="alert" :size="15" />{{ error }}</p></div>
        <div v-if="state.preview.profiles.length" class="preview-table table-scroll"><table><thead><tr><th><input type="checkbox" aria-label="选择全部连接" :checked="selected.length === state.preview.profiles.length" :disabled="!!state.mutation" @change="toggleAll" /></th><th>服务器</th><th>账号</th><th>导入检查</th></tr></thead><tbody><tr v-for="profile in state.preview.profiles" :key="profile.id"><td><input v-model="selected" type="checkbox" :value="profile.id" :aria-label="`导入 ${profile.name}`" :disabled="!!state.mutation" /></td><td><strong>{{ profile.name }}</strong><small class="mono">{{ profile.host }}:{{ profile.port }}</small></td><td class="mono">{{ profile.username }}</td><td><span v-if="conflicts.some((item) => item.id === profile.id)" class="badge amber">名称或 ID 冲突</span><span v-else-if="needsCredential(profile)" class="badge amber">待补凭据</span><span v-else class="badge green">配置就绪</span></td></tr></tbody></table></div>
        <div v-else class="preview-empty">未找到可导入的连接，请查看上方解析提示。</div>
        <div class="strategy-row"><label class="field"><span>导入方式</span><select v-model="strategy" :disabled="!!state.mutation"><option value="merge">合并到现有连接</option><option value="replace">替换全部连接</option></select></label><p>{{ strategy === 'replace' ? `将替换当前 ${state.profiles.length} 台服务器的配置，请确认。` : '保留其他连接；同名或同 ID 由后端合并。' }}</p></div>
        <div v-if="inspection.needsCredential.length" class="inline-note warning"><Icon name="key" :size="16" />{{ inspection.needsCredential.length }} 台缺少凭据；导入后请在编辑窗口补齐，再测试连接。</div>
        <label v-if="risky" class="check-row acknowledgement"><input v-model="acknowledge" type="checkbox" :disabled="!!state.mutation" /><span>我已检查上述提示{{ strategy === 'replace' ? '，确认替换全部连接' : '与冲突' }}</span></label>
        <div class="import-submit"><span>已选择 {{ picked.length }} 台服务器</span><button class="button primary" :disabled="!canImport" @click="dock.applyImport(picked, strategy)"><span v-if="state.mutation === 'import'" class="spinner" /><Icon v-else name="download" :size="16" />{{ state.mutation === 'import' ? '导入中…' : '确认导入' }}</button></div>
      </template>
      <div v-else class="preview-placeholder"><Icon name="download" :size="26" /><p>选择文件后，连接预览会显示在这里。</p><small>先检查，再导入。</small></div>
    </section>
    <aside class="import-aside"><section class="card detected-panel"><div class="panel-heading"><span>检测到的来源</span><button class="icon-button" aria-label="重新扫描来源" :disabled="!state.desktop || !state.ready || state.discoveryBusy" @click="dock.discoverSources()"><Icon name="refresh" :size="16" /></button></div><div v-if="state.discoveryBusy" class="small-empty"><span class="spinner" />扫描中…</div><template v-else><button v-for="(source, index) in state.sources" :key="`${source.path}-${index}`" class="detected-source" :disabled="!source.exists || !!state.mutation || picking" @click="useDetected(source.kind, source.path)"><span class="source-logo small"><Icon name="folder" :size="18" /></span><span><strong>{{ source.label }}</strong><small class="break-text">{{ source.path }}</small><em>{{ source.exists ? source.profileCount === undefined ? '已找到配置' : `${source.profileCount} 个连接` : '未找到文件' }}</em></span><Icon name="chevron" :size="15" /></button><div v-if="!state.sources.length" class="small-empty">{{ state.desktop ? '未检测到本机配置，可手动选择文件。' : '桌面启动后扫描本机连接配置。' }}</div></template></section><section class="import-tip"><span class="tip-icon"><Icon name="shield" :size="23" /></span><h3>配置迁移，凭据留在本机</h3><p>Xshell 加密密码不能直接使用；Tabby 保险库等限制会在预览中说明。</p><p>导入后补齐缺少的密码或私钥，通过真实连接测试确认可用。</p><span><Icon name="check" :size="14" />文件不会上传到云端</span></section></aside>
  </div>
</template>
