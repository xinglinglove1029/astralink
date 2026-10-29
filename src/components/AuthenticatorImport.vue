<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { activeBridge } from '../api'
import type { CommandMap } from '../types'
import { readAuthenticatorFile } from '../authenticatorImport'
import Modal from './Modal.vue'
import Icon from './Icon.vue'

const emit = defineEmits<{ close: []; selected: [secret: string, label: string] }>()
const content = ref(''), filename = ref(''), password = ref(''), selected = ref(''), error = ref('')
const accounts = ref<CommandMap['read_authenticator_backup']['result']['accounts']>([])
const busy = ref(false)
const chosen = computed(() => accounts.value.find(account => account.id === selected.value && account.supported))
let active = true
function clearAccounts() { accounts.value = []; selected.value = ''; error.value = '' }
async function chooseFile(event: Event) {
  const input = event.currentTarget as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  clearAccounts(); content.value = ''; filename.value = ''; password.value = ''; busy.value = true
  try { const text = await readAuthenticatorFile(file); if (active) { content.value = text; filename.value = file.name } }
  catch (cause) { if (active) error.value = cause instanceof Error ? cause.message : '无法读取备份文件' }
  finally { busy.value = false }
}
async function preview() {
  if (busy.value || !content.value) return
  clearAccounts(); busy.value = true
  try {
    const result = await activeBridge.invoke('read_authenticator_backup', { content: content.value, password: password.value || undefined })
    if (active) accounts.value = result.accounts
  } catch (cause) { if (active) error.value = cause instanceof Error ? cause.message : '无法解析备份，请检查文件和密码' }
  finally { busy.value = false }
}
async function selectAccount() {
  if (busy.value || !chosen.value) return
  const label = [chosen.value.issuer, chosen.value.account].filter(Boolean).join(' · ') || '认证器账号'
  busy.value = true; error.value = ''
  try {
    const result = await activeBridge.invoke('read_authenticator_backup', { content: content.value, password: password.value || undefined, entryId: selected.value })
    if (active) {
      if (!result.secret) throw new Error('未能读取选中账号，请重新预览')
      emit('selected', result.secret, label)
    }
  } catch (cause) { if (active) error.value = cause instanceof Error ? cause.message : '无法导入选中账号' }
  finally { busy.value = false }
}
onUnmounted(() => { active = false; content.value = ''; password.value = ''; accounts.value = []; selected.value = '' })
</script>

<template>
  <Modal title="导入身份验证器" subtitle="选择备份中的一个账号，绑定到当前连接。" :busy="busy" wide @close="emit('close')">
    <div class="auth-import">
      <div class="import-guide"><Icon name="info" :size="18" /><p>在 Chrome 身份验证器中打开“设置 → 备份”，下载普通备份或密码保护备份。支持 Authenticator Extension 的 JSON、TXT 格式，原插件中的账号会保留。</p></div>
      <div class="import-fields">
        <label class="field"><span>认证器备份</span><input type="file" accept=".json,.txt,application/json,text/plain" :disabled="busy" @change="chooseFile" /><small>{{ filename || '选择 1 MB 以内的备份文件' }}</small></label>
        <label class="field"><span>备份密码 <small>仅加密备份需要</small></span><input v-model="password" type="password" autocomplete="off" :disabled="busy" placeholder="导出备份时使用的密码" @input="clearAccounts" /></label>
      </div>
      <button type="button" class="button secondary" :disabled="busy || !content" @click="preview"><span v-if="busy" class="spinner" />{{ busy ? '处理中…' : '读取账号' }}</button>
      <div v-if="accounts.length" class="import-accounts">
        <div class="account-caption"><strong>选择账号</strong><span>{{ accounts.length }} 个账号 · 仅导入选中项</span></div>
        <div class="account-scroll"><table><thead><tr><th class="pick-column"></th><th>服务</th><th>账号</th><th>兼容状态</th></tr></thead><tbody>
          <tr v-for="account in accounts" :key="account.id" :class="{ chosen: selected === account.id, unsupported: !account.supported }">
            <td><input v-model="selected" type="radio" name="authenticator-account" :value="account.id" :disabled="busy || !account.supported" :aria-label="`选择 ${account.issuer} ${account.account}`" /></td>
            <td>{{ account.issuer || '未命名服务' }}</td><td>{{ account.account || '未命名账号' }}</td>
            <td><span :class="account.supported ? 'supported-label' : 'unsupported-label'">{{ account.supported ? '可自动验证' : account.reason }}</span></td>
          </tr>
        </tbody></table></div>
      </div>
      <p v-if="error" class="import-error" role="alert">{{ error }}</p>
      <p class="import-note">预览不显示动态密钥。选择账号后，点击连接编辑窗口的“保存”才会写入凭据库。备份密码不保存。</p>
    </div>
    <template #footer><button type="button" class="button secondary" :disabled="busy" @click="emit('close')">取消</button><button type="button" class="button primary" :disabled="busy || !chosen" @click="selectAccount">使用选中账号</button></template>
  </Modal>
</template>

<style scoped>
.auth-import { display: grid; gap: 18px; }
.import-guide { display: flex; gap: 10px; align-items: flex-start; padding: 12px 14px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-muted); color: var(--text-muted); font-size: 13px; line-height: 1.7; }
.import-guide p { margin: 0; }.import-guide svg { flex-shrink: 0; margin-top: 3px; }
.import-fields { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }.field small { color: var(--text-muted); font-weight: 400; }
.auth-import > button { justify-self: start; }.import-accounts { border: 1px solid var(--border); border-radius: 8px; overflow: hidden; }
.account-caption { display: flex; justify-content: space-between; padding: 12px 16px; gap: 12px; border-bottom: 1px solid var(--border); font-size: 13px; }.account-caption span { color: var(--text-muted); }
.account-scroll { max-height: 280px; overflow: auto; }table { width: 100%; border-collapse: collapse; font-size: 13px; }th, td { padding: 12px; text-align: left; border-bottom: 1px solid var(--border); overflow-wrap: anywhere; }th { color: var(--text-muted); font-weight: 500; background: var(--surface-muted); position: sticky; top: 0; }.pick-column { width: 36px; }tr:last-child td { border-bottom: 0; }tr.chosen { background: var(--surface-muted); }tr.unsupported { color: var(--text-muted); }td input { accent-color: var(--accent, #2563eb); }.supported-label { color: #17805a; }.unsupported-label { font-size: 12px; }.import-note { color: var(--text-muted); font-size: 12px; line-height: 1.7; margin: 0; }.import-error { color: var(--danger, #b42318); background: #fff4f2; padding: 10px 12px; border-radius: 6px; margin: 0; font-size: 13px; }
@media(max-width: 680px) { .import-fields { grid-template-columns: 1fr; }.account-caption { flex-direction: column; } }
</style>
