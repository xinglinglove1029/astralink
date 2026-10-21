<script setup lang="ts">
import { computed, onUnmounted, reactive, ref } from 'vue'
import Modal from './Modal.vue'
import Icon from './Icon.vue'
import SecretInput from './SecretInput.vue'
import { createProfile, normalizeProfile, stripSecrets, validateProfile } from '../domain'
import { readTotpSecretFromImage } from '../totpQr'
import type { ConnectionProfile, SecretField } from '../types'
const props = defineProps<{ profile?: ConnectionProfile; profiles: ConnectionProfile[]; busy: boolean; error?: string; pickFile: () => Promise<string | null>; loadSecret: (profileId: string, field: SecretField) => Promise<string | null> }>()
const emit = defineEmits<{ close: []; save: [profile: ConnectionProfile] }>()
const form = reactive<ConnectionProfile>(props.profile ? { ...stripSecrets(props.profile) } : createProfile())
const errors = ref<Record<string, string>>({})
const advanced = ref(!!(form.hasTotpSecret || form.jumpHostId || form.targetHost || form.agent || form.notes || form.allowedLocalPaths?.length || form.allowedRemotePaths?.length || form.transportMode === 'shell'))
const localPaths = ref(form.allowedLocalPaths?.join('\n') ?? '')
const remotePaths = ref(form.allowedRemotePaths?.join('\n') ?? '')
const picking = ref(false)
const qrInput = ref<HTMLInputElement>()
const qrBusy = ref(false)
const qrMessage = ref('')
const qrError = ref('')
const groups = computed(() => [...new Set(props.profiles.map((profile) => profile.group).filter(Boolean))])
function loadSaved(field: SecretField) {
  const id = props.profile?.id
  return id ? () => props.loadSecret(id, field) : undefined
}
let active = true
async function chooseKey() {
  picking.value = true
  try { const file = await props.pickFile(); if (active && file) form.privateKeyPath = file }
  catch { if (active) errors.value.privateKeyPath = '文件选择失败，请输入路径' }
  finally { picking.value = false }
}
async function readQr(event: Event) {
  const input = event.currentTarget as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || qrBusy.value) return
  qrBusy.value = true
  qrMessage.value = ''
  qrError.value = ''
  try {
    const seed = await readTotpSecretFromImage(file)
    if (active) {
      form.totpSecret = seed
      delete errors.value.totpSecret
      qrMessage.value = '已识别并填入动态密钥，请保存服务器'
    }
  } catch (error) {
    if (active) qrError.value = error instanceof Error ? error.message : '二维码识别失败'
  } finally { qrBusy.value = false }
}
function submit() {
  form.allowedLocalPaths = localPaths.value.split(/\r?\n/).map((path) => path.trim()).filter(Boolean)
  form.allowedRemotePaths = remotePaths.value.split(/\r?\n/).map((path) => path.trim()).filter(Boolean)
  errors.value = validateProfile(form, props.profiles)
  if (errors.value.jumpHostId || errors.value.targetHost || errors.value.targetUsername || errors.value.allowedLocalPaths || errors.value.allowedRemotePaths) advanced.value = true
  if (!Object.keys(errors.value).length) emit('save', normalizeProfile(form))
}
onUnmounted(() => { active = false; form.password = undefined; form.passphrase = undefined; form.totpSecret = undefined; qrMessage.value = ''; qrError.value = '' })
</script>
<template>
  <Modal :title="profile ? '编辑服务器' : '添加服务器'" subtitle="保存连接信息，凭据交由系统凭据库管理。" :busy="busy || picking || qrBusy" wide @close="emit('close')">
    <form id="profile-form" class="profile-form" novalidate @submit.prevent="submit">
      <fieldset :disabled="busy || qrBusy"><legend class="sr-only">服务器配置</legend>
        <div class="form-grid">
          <label class="field"><span>名称 <b>*</b></span><input v-model="form.name" autofocus placeholder="例如：生产应用服务器" maxlength="120" :aria-invalid="!!errors.name" /><small v-if="errors.name" class="field-error">{{ errors.name }}</small></label>
          <label class="field"><span>分组</span><input v-model="form.group" list="host-groups" placeholder="例如：生产环境" maxlength="120" /><datalist id="host-groups"><option v-for="group in groups" :key="group" :value="group" /></datalist></label>
          <label class="field"><span>主机地址 <b>*</b></span><input v-model="form.host" placeholder="IP 或主机名" spellcheck="false" :aria-invalid="!!errors.host" /><small v-if="errors.host" class="field-error">{{ errors.host }}</small></label>
          <label class="field"><span>端口 <b>*</b></span><input v-model.number="form.port" type="number" min="1" max="65535" step="1" :aria-invalid="!!errors.port" /><small v-if="errors.port" class="field-error">{{ errors.port }}</small></label>
          <label class="field"><span>用户名 <b>*</b></span><input v-model="form.username" placeholder="例如：deploy" autocomplete="off" spellcheck="false" :aria-invalid="!!errors.username" /><small v-if="errors.username" class="field-error">{{ errors.username }}</small></label>
          <label class="field"><span>认证方式</span><select v-model="form.authType"><option value="password">密码</option><option value="privateKey">私钥</option><option value="agent">SSH Agent</option></select></label>
          <label v-if="form.authType === 'password'" class="field span-two"><span>密码</span><SecretInput v-model="form.password" :load-saved="form.hasPassword ? loadSaved('password') : undefined" :placeholder="form.hasPassword ? '已保存密码；留空保留现有凭据' : '输入登录密码'" autocomplete="new-password" :aria-invalid="!!errors.password" /><small v-if="errors.password" class="field-error">{{ errors.password }}</small></label>
          <template v-if="form.authType === 'privateKey'"><label class="field span-two"><span>私钥路径 <b>*</b></span><div class="input-action"><input v-model="form.privateKeyPath" placeholder="选择本机私钥文件" :aria-invalid="!!errors.privateKeyPath" /><button type="button" class="button secondary" :disabled="picking" @click="chooseKey"><Icon name="folder" :size="16" />选择</button></div><small v-if="errors.privateKeyPath" class="field-error">{{ errors.privateKeyPath }}</small></label><label class="field span-two"><span>私钥口令</span><SecretInput v-model="form.passphrase" reveal-label="私钥口令" :load-saved="form.hasPassphrase ? loadSaved('passphrase') : undefined" :placeholder="form.hasPassphrase ? '已保存口令；留空保留' : '未加密私钥可留空'" autocomplete="new-password" /></label></template>
          <label v-if="form.authType === 'agent'" class="field span-two"><span>Agent 路径</span><input v-model="form.agent" placeholder="Pageant 或系统 Agent 管道路径" spellcheck="false" /><small v-if="errors.agent" class="field-error">{{ errors.agent }}</small></label>
        </div>
        <label class="check-row"><input v-model="form.tryKeyboard" type="checkbox" /><span>启用交互认证<small>服务器请求验证码时弹窗输入</small></span></label>
        <div v-if="form.tryKeyboard || form.hasTotpSecret" class="field totp-config"><span><label for="totp-secret">动态验证码密钥</label></span><div class="totp-input-row"><SecretInput id="totp-secret" v-model="form.totpSecret" reveal-label="动态验证码密钥" :load-saved="form.hasTotpSecret ? loadSaved('totpSecret') : undefined" autocomplete="new-password" spellcheck="false" :placeholder="form.hasTotpSecret ? '已保存密钥；留空保留' : 'Base32 密钥，可从认证器绑定信息获取'" :aria-invalid="!!errors.totpSecret" /><button type="button" class="button secondary" :disabled="qrBusy" @click="qrInput?.click()"><span v-if="qrBusy" class="spinner" /><Icon v-else name="scan" :size="15" />识别二维码</button><button v-if="form.hasTotpSecret" type="button" class="button secondary danger-text" :disabled="qrBusy" @click="form.totpSecret = ''; form.hasTotpSecret = false; qrMessage = ''">清除密钥</button></div><input ref="qrInput" class="sr-only" type="file" accept="image/png,image/jpeg,image/webp,image/gif,image/bmp" aria-label="选择二维码图片" tabindex="-1" @change="readQr" /><small v-if="errors.totpSecret" class="field-error">{{ errors.totpSecret }}</small><small v-else-if="qrError" class="field-error" role="alert">{{ qrError }}</small><small v-else-if="qrMessage" class="field-success" role="status">{{ qrMessage }}</small><small v-else>识别后点击保存；密钥仅存于系统凭据库。</small></div>
        <label class="check-row"><input v-model="form.enabled" type="checkbox" /><span>启用此连接<small>允许 MCP 访问这台服务器</small></span></label>
        <button type="button" class="advanced-toggle" :aria-expanded="advanced" @click="advanced = !advanced"><Icon :name="advanced ? 'chevronDown' : 'chevron'" :size="15" />高级配置</button>
        <div v-show="advanced" class="form-grid advanced-fields">
          <label class="field"><span>传输方式</span><select v-model="form.transportMode"><option value="exec">命令执行</option><option value="shell">交互 Shell</option></select></label>
          <label class="field"><span>跳板机</span><select v-model="form.jumpHostId"><option :value="undefined">无</option><option v-for="item in profiles.filter((item) => item.id !== form.id)" :key="item.id" :value="item.id">{{ item.name }}</option></select><small v-if="errors.jumpHostId" class="field-error">{{ errors.jumpHostId }}</small></label>
          <label class="field"><span>目标资产</span><input v-model="form.targetHost" placeholder="JumpServer 资产地址" :aria-invalid="!!errors.targetHost" /><small v-if="errors.targetHost" class="field-error">{{ errors.targetHost }}</small></label>
          <label class="field"><span>目标账号</span><input v-model="form.targetUsername" placeholder="资产登录账号" :aria-invalid="!!errors.targetUsername" /><small v-if="errors.targetUsername" class="field-error">{{ errors.targetUsername }}</small></label>
          <label class="field span-two"><span>本地文件目录</span><textarea v-model="localPaths" rows="2" placeholder="允许上传、下载的本地目录，每行一个绝对路径" /><small v-if="errors.allowedLocalPaths" class="field-error">{{ errors.allowedLocalPaths }}</small></label>
          <label class="field span-two"><span>远端文件目录</span><textarea v-model="remotePaths" rows="2" placeholder="需要限制远端路径时填写，每行一个目录" /><small v-if="errors.allowedRemotePaths" class="field-error">{{ errors.allowedRemotePaths }}</small></label>
          <label class="field span-two"><span>备注</span><textarea v-model="form.notes" rows="2" placeholder="用途或其他连接说明" maxlength="1000" /></label>
        </div>
      </fieldset>
      <p v-if="error" class="field-error" role="alert">{{ error }}</p>
      <div class="inline-note"><Icon name="shield" :size="16" />密码和口令不会显示在列表或诊断信息中。</div>
    </form>
    <template #footer><button class="button secondary" :disabled="busy || picking || qrBusy" @click="emit('close')">取消</button><button class="button primary" type="submit" form="profile-form" :disabled="busy || picking || qrBusy"><span v-if="busy" class="spinner" /><Icon v-else name="check" :size="16" />{{ busy ? '保存中…' : '保存服务器' }}</button></template>
  </Modal>
</template>
