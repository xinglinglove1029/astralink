<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { desktopBridge } from '../api'
import type { AstralinkController } from '../controller'
import Icon from './Icon.vue'

const props = defineProps<{ dock: AstralinkController }>()
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
    <div class="section-heading"><span class="section-icon"><Icon name="server" :size="21" /></span><div><h3>远端备份</h3><p>WebDAV 加密快照，手动上传与恢复。</p></div></div>
    <div class="backup-fields">
      <label class="field"><span>HTTPS WebDAV 文件地址</span><input v-model.trim="form.url" :disabled="busy" type="url" placeholder="https://dav.example.com/astralink.backup" autocomplete="off" /></label>
      <label class="field"><span>账号</span><input v-model="form.username" :disabled="busy" autocomplete="off" /></label>
      <label class="field"><span>WebDAV 密码</span><input v-model="form.password" :disabled="busy" type="password" autocomplete="off" /></label>
      <label class="field"><span>备份口令</span><input v-model="form.passphrase" :disabled="busy" type="password" autocomplete="off" minlength="12" /></label>
    </div>
    <p class="storage-note">地址和账号仅保存在本机；WebDAV 密码与备份口令不保存。备份口令丢失后无法解密；快照包含连接凭据，不包含本机私钥文件、MCP 令牌及应用设置。</p>
    <div class="button-row backup-actions">
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !form.url || form.passphrase.length < 12" @click="read">查看远端</button>
      <button class="button primary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !form.url || form.passphrase.length < 12 || (!!preview && !overwrite)" @click="upload">加密上传</button>
      <button class="button secondary" :disabled="busy || !dock.state.desktop || !dock.state.ready || !preview || !confirmRestore" @click="restore">恢复连接</button>
    </div>
    <label v-if="preview" class="check-row"><input v-model="overwrite" type="checkbox" />覆盖已预览的远端版本</label>
    <label v-if="preview" class="check-row"><input v-model="confirmRestore" type="checkbox" />确认用远端 {{ preview.serverCount }} 台连接替换本机连接</label>
    <p v-if="message" class="storage-note" role="status">{{ message }}</p>
  </section>
</template>

<style scoped>
.backup-panel { grid-column: 1 / -1; }
.backup-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
.backup-fields .field:first-child { grid-column: 1 / -1; }
.backup-actions { display: flex; flex-wrap: wrap; gap: 8px; margin: 16px 0 10px; }
.check-row { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 12px; }
@media (max-width: 720px) { .backup-fields { grid-template-columns: 1fr; } }
</style>
