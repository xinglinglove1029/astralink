<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { activeBridge, desktopBridge } from '../api'
import { webMode } from '../webBridge'
import type { AstraLinkController } from '../controller'
import Icon from './Icon.vue'

const props = defineProps<{ dock: AstraLinkController }>()
function savedTarget(): { url: string; username: string } {
  try {
    const value = JSON.parse(localStorage.getItem('astralink-webdav-target') ?? '{}')
    return { url: typeof value.url === 'string' ? value.url : '', username: typeof value.username === 'string' ? value.username : '' }
  } catch { return { url: '', username: '' } }
}
const form = reactive({ ...savedTarget(), password: '', passphrase: '' })
const preview = ref<{ fingerprint: string; serverCount: number }>()
const overwrite = ref(false)
const confirmRestore = ref(false)
const busy = ref(false)
const message = ref('')
const localFile = ref('')
const localPreview = ref<{ fingerprint: string; serverCount: number; settings?: { mcpPort: number }; needsAttention: number }>()
const localStrategy = ref<'merge' | 'replace'>('merge')
const confirmLocal = ref(false)
watch(() => form.passphrase, () => { localPreview.value = undefined; confirmLocal.value = false })

async function exportLocal() {
  if (busy.value || form.passphrase.length < 12) return
  const filePath = await activeBridge.saveBackupFile?.()
  if (!filePath) return
  busy.value = true
  try {
    await activeBridge.invoke('export_backup_file', { filePath, passphrase: form.passphrase })
    message.value = '加密备份已导出。'
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}
async function chooseLocal() {
  const filePath = await activeBridge.pickBackupFile?.()
  if (filePath) { localFile.value = filePath; localPreview.value = undefined; confirmLocal.value = false }
}
async function previewLocal() {
  if (busy.value || !localFile.value) return
  busy.value = true
  try {
    localPreview.value = await activeBridge.invoke('preview_backup_file', { filePath: localFile.value, passphrase: form.passphrase })
    message.value = `备份包含 ${localPreview.value.serverCount} 台连接。`
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}
async function restoreLocal() {
  if (busy.value || !confirmLocal.value || !localPreview.value) return
  busy.value = true
  try {
    await activeBridge.invoke('restore_backup_file', { filePath: localFile.value, passphrase: form.passphrase, expectedFingerprint: localPreview.value.fingerprint, strategy: localStrategy.value })
    await props.dock.refreshAppState()
    message.value = '备份已导入。'
    localPreview.value = undefined; confirmLocal.value = false
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}

watch(form, () => { preview.value = undefined; overwrite.value = false; confirmRestore.value = false; message.value = '' })
watch(() => [form.url, form.username], () => {
  if (!form.url) { localStorage.removeItem('astralink-webdav-target'); return }
  try {
    const parsed = new URL(form.url)
    if (parsed.protocol === 'https:' && !parsed.username && !parsed.password && !parsed.search && !parsed.hash) {
      localStorage.setItem('astralink-webdav-target', JSON.stringify({ url: form.url, username: form.username }))
    }
  } catch { /* Incomplete address stays in the input only. */ }
})
const args = () => ({ ...form })
async function read() {
  if (busy.value) return
  busy.value = true
  message.value = ''
  try {
    preview.value = await desktopBridge.invoke('preview_webdav_backup', args())
    message.value = `远端有 ${preview.value.serverCount} 台连接，请核对后操作。`
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}
async function upload() {
  if (busy.value || (preview.value && !overwrite.value)) return
  busy.value = true
  message.value = ''
  try {
    const result = await desktopBridge.invoke('backup_to_webdav', { ...args(), ...(preview.value ? { expectedFingerprint: preview.value.fingerprint } : {}) })
    preview.value = result
    message.value = `已加密上传 ${result.serverCount} 台连接。`
    overwrite.value = false
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}
async function restore() {
  if (busy.value || !preview.value || !confirmRestore.value) return
  busy.value = true
  message.value = ''
  try {
    const restored = await desktopBridge.invoke('restore_webdav_backup', { ...args(), expectedFingerprint: preview.value.fingerprint })
    await props.dock.refreshAppState()
    message.value = `已恢复 ${restored.length} 台连接。`
    confirmRestore.value = false
  } catch (error) { message.value = String(error) }
  finally { busy.value = false }
}
</script>

<template>
  <section class="card settings-card backup-panel">
    <div class="section-heading"><span class="section-icon"><Icon name="download" :size="21" /></span><div><h3>配置迁移</h3><p>导出或导入 AstraLink 加密备份。</p></div></div>
    <label class="field"><span>备份口令</span><input v-model="form.passphrase" type="password" autocomplete="off" minlength="12" :disabled="busy" /></label>
    <div class="button-row backup-actions">
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready || form.passphrase.length < 12" @click="exportLocal"><Icon name="download" :size="15" />导出备份</button>
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready" @click="chooseLocal"><Icon name="folder" :size="15" />选择备份</button>
      <button class="button secondary" :disabled="busy || !localFile || form.passphrase.length < 12" @click="previewLocal">预览导入</button>
    </div>
    <p v-if="localFile" class="storage-note mono break-text">{{ localFile }}</p>
    <template v-if="localPreview">
      <p class="storage-note">{{ localPreview.serverCount }} 台连接{{ localPreview.settings ? `；包含应用设置（MCP 端口 ${localPreview.settings.mcpPort}）` : '；旧版备份无应用设置' }}。{{ localPreview.needsAttention ? `${localPreview.needsAttention} 台使用当前机器不存在的私钥或传输目录，导入后将停用并需重新配置。` : '私钥文件不会打包。' }}</p>
      <label class="field"><span>导入方式</span><select v-model="localStrategy" :disabled="busy"><option value="merge">合并连接</option><option value="replace">替换所有连接</option></select></label>
      <div class="backup-restore-actions">
        <label class="check-row"><input v-model="confirmLocal" type="checkbox" />确认导入并应用可迁移设置</label>
        <button class="button primary" :disabled="busy || !confirmLocal" @click="restoreLocal"><Icon name="download" :size="15" />导入备份</button>
      </div>
    </template>
    <p class="storage-note">备份含连接凭据与动态密钥，不含私钥文件、MCP 令牌和登录信息。仅在受信任设备保存备份。</p>
    <template v-if="!webMode"><div class="section-heading"><span class="section-icon"><Icon name="server" :size="21" /></span><div><h3>远端备份</h3><p>WebDAV 加密快照，手动上传与恢复。</p></div></div>
    <div class="backup-fields">
      <label class="field"><span>HTTPS WebDAV 文件地址</span><input v-model.trim="form.url" :disabled="busy" type="url" placeholder="https://dav.example.com/astralink.backup" autocomplete="off" /></label>
      <label class="field"><span>账号</span><input v-model="form.username" :disabled="busy" autocomplete="off" /></label>
      <label class="field"><span>WebDAV 密码</span><input v-model="form.password" :disabled="busy" type="password" autocomplete="off" /></label>
    </div>
    <p class="storage-note">地址和账号仅保存在本机；WebDAV 密码与备份口令不保存。备份口令丢失后无法解密；新版快照包含应用设置，旧版仍可恢复。</p>
    <div class="button-row backup-actions">
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !form.url || form.passphrase.length < 12" @click="read">查看远端</button>
      <button class="button primary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !form.url || form.passphrase.length < 12 || (!!preview && !overwrite)" @click="upload">加密上传</button>
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !preview || !confirmRestore" @click="restore">恢复连接</button>
    </div>
    <label v-if="preview" class="check-row"><input v-model="overwrite" type="checkbox" />覆盖已预览的远端版本</label>
    <label v-if="preview" class="check-row"><input v-model="confirmRestore" type="checkbox" />确认用远端 {{ preview.serverCount }} 台连接替换本机连接</label></template>
    <p v-if="message" class="storage-note" role="status">{{ message }}</p>
  </section>
</template>

<style scoped>
.backup-panel { grid-column: 1 / -1; }
.backup-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.backup-fields .field:first-child { grid-column: 1 / -1; }
.backup-actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 16px 0 10px; }
.check-row { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 12px; }
.backup-restore-actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 12px 20px; margin: 20px 0 14px; padding-top: 16px; border-top: 1px solid var(--border, #dfe6ef); }
.backup-restore-actions .check-row { flex: 1 1 240px; min-width: 0; margin: 0; }
.backup-restore-actions .button { flex: 0 0 auto; }
@media (max-width: 720px) {
  .backup-fields { grid-template-columns: 1fr; }
  .backup-restore-actions { align-items: stretch; flex-direction: column; }
  .backup-restore-actions .check-row { flex: 0 0 auto; }
  .backup-restore-actions .button { width: 100%; }
}
</style>
