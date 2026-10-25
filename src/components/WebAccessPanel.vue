<script setup lang="ts">
import { copyText } from '../clipboard'
import { computed, onMounted, reactive, ref } from 'vue'
import { currentWebUser, webManagement, type WebUser } from '../webBridge'
import { webRoleName, type WebRole } from '../webRoles'
import Icon from './Icon.vue'
import Modal from './Modal.vue'

const users = ref<WebUser[]>([])
const query = ref('')
const loading = ref(false)
const busy = ref(false)
const notice = ref('')
const error = ref('')
const editing = ref(false)
const pendingAction = ref<{ kind: 'delete' | 'revoke'; user: WebUser } | null>(null)
const tokenTarget = ref<WebUser | null>(null)
const issuedToken = ref('')
const tokenCopied = ref(false)
const form = reactive<{ id: string; username: string; password: string; role: WebRole | null; enabled: boolean }>({
  id: '', username: '', password: '', role: 'viewer', enabled: true,
})
const filtered = computed(() => users.value.filter(user =>
  `${user.username} ${webRoleName(user.role)}`.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase()),
))

async function refresh() {
  if (currentWebUser.value?.role !== 'admin') return
  loading.value = true
  error.value = ''
  try { users.value = await webManagement<WebUser[]>('list_web_users') }
  catch (cause) { error.value = cause instanceof Error ? cause.message : '读取用户失败' }
  finally { loading.value = false }
}
function openCreate() {
  Object.assign(form, { id: '', username: '', password: '', role: 'viewer', enabled: true })
  error.value = ''
  editing.value = true
}
function openEdit(user: WebUser) {
  Object.assign(form, { id: user.id, username: user.username, password: '', role: user.role, enabled: user.enabled })
  error.value = ''
  editing.value = true
}
function close() { if (!busy.value) { editing.value = false; form.password = '' } }
async function save() {
  if (busy.value) return
  busy.value = true
  error.value = ''
  try {
    const args = { id: form.id, username: form.username, password: form.password || undefined, enabled: form.enabled }
    await webManagement(form.id ? 'save_web_user_account' : 'save_web_user', form.id ? args : { ...args, role: 'viewer' })
    editing.value = false
    form.password = ''
    notice.value = form.id ? '用户已更新，旧会话和令牌已失效' : '用户已创建'
    await refresh()
  } catch (cause) { error.value = cause instanceof Error ? cause.message : '保存失败' }
  finally { busy.value = false }
}
async function confirmAction() {
  const action = pendingAction.value
  if (busy.value || !action) return
  busy.value = true
  try {
    await webManagement(action.kind === 'delete' ? 'delete_web_user' : 'revoke_web_token', { id: action.user.id })
    pendingAction.value = null
    notice.value = action.kind === 'delete' ? '用户已删除' : '令牌已撤销'
    await refresh()
  } catch (cause) { error.value = cause instanceof Error ? cause.message : '操作失败' }
  finally { busy.value = false }
}
function openToken(user: WebUser) {
  if (busy.value || !user.enabled || !['admin', 'operator'].includes(user.role ?? '')) return
  error.value = ''
  issuedToken.value = ''
  tokenCopied.value = false
  tokenTarget.value = user
}
function closeToken() {
  if (busy.value) return
  issuedToken.value = ''
  tokenCopied.value = false
  tokenTarget.value = null
}
async function issueToken() {
  const target = tokenTarget.value
  if (busy.value || !target || issuedToken.value) return
  busy.value = true
  error.value = ''
  try {
    issuedToken.value = await webManagement<string>('issue_web_user_token', { id: target.id })
    await refresh()
  } catch (cause) { error.value = cause instanceof Error ? cause.message : '签发失败' }
  finally { busy.value = false }
}
async function copyToken() {
  if (!issuedToken.value) return
  error.value = ''
  try { await copyText(issuedToken.value); tokenCopied.value = true }
  catch { error.value = '复制失败，请手动复制令牌' }
}
onMounted(refresh)
</script>

<template>
  <section class="web-access">
    <div class="access-toolbar">
      <label class="access-search"><Icon name="search" :size="16" /><span class="sr-only">搜索用户或角色</span><input v-model="query" type="search" placeholder="搜索用户或角色" /></label>
      <span class="access-count">共 {{ users.length }} 位用户</span>
      <button class="button secondary small" :disabled="loading || busy" aria-label="刷新用户列表" title="刷新用户列表" @click="refresh"><Icon name="refresh" :size="15" /></button>
      <button class="button primary" :disabled="loading || busy" @click="openCreate"><Icon name="plus" :size="15" />新增用户</button>
    </div>
    <p v-if="error && !editing" class="notice error" role="alert">{{ error }}</p>
    <p v-if="notice" class="access-notice" role="status">{{ notice }}</p>
    <div class="access-table-wrap">
      <table class="access-table">
        <thead><tr><th scope="col">用户名</th><th scope="col">角色</th><th scope="col">账号状态</th><th scope="col">MCP 令牌</th><th scope="col" class="action-cell">操作</th></tr></thead>
        <tbody>
          <tr v-for="user in filtered" :key="user.id">
            <td><strong>{{ user.username }}</strong><small v-if="user.id === 'admin'">初始管理员</small><small class="mobile-state">{{ user.enabled ? '已启用' : '已停用' }} · MCP {{ user.hasToken ? '已签发' : '未签发' }}</small></td>
            <td>{{ webRoleName(user.role) }}</td>
            <td><span :class="['state-label', user.enabled ? 'is-enabled' : 'is-disabled']"><i />{{ user.enabled ? '已启用' : '已停用' }}</span></td>
            <td>{{ user.hasToken ? '已签发' : '未签发' }}</td>
            <td class="action-cell"><div class="row-actions"><button v-if="user.id !== 'admin'" class="text-button" :disabled="busy" :aria-label="`编辑 ${user.username}`" @click="openEdit(user)">编辑</button><button v-if="user.role === 'admin' || user.role === 'operator'" class="text-button" :disabled="busy || !user.enabled" :title="!user.enabled ? '启用账号后可签发' : undefined" :aria-label="`${user.hasToken ? '重新签发' : '签发'} ${user.username} 的 MCP 令牌`" @click="openToken(user)">{{ user.hasToken ? '重新签发' : '签发令牌' }}</button><button v-if="user.hasToken" class="text-button" :disabled="busy" :aria-label="`撤销 ${user.username} 的令牌`" @click="error = ''; pendingAction = { kind: 'revoke', user }">撤销</button><button v-if="user.id !== 'admin'" class="text-button danger-text" :disabled="busy" :aria-label="`删除 ${user.username}`" @click="error = ''; pendingAction = { kind: 'delete', user }">删除</button></div></td>
          </tr>
          <tr v-if="!filtered.length"><td colspan="5" class="empty-row">{{ loading ? '正在读取用户…' : query ? '没有匹配的用户' : '暂无用户' }}</td></tr>
        </tbody>
      </table>
    </div>
    <Modal v-if="editing" :title="form.id ? '编辑用户' : '新增用户'" :subtitle="form.id ? '保存账号资料后，旧会话与个人令牌将失效。' : '新账号默认分配查看者角色。'" :busy="busy" @close="close">
      <form id="web-user-form" class="user-form" @submit.prevent="save">
        <label class="field"><span>用户名</span><input v-model.trim="form.username" autocomplete="off" required minlength="2" maxlength="32" pattern="[a-zA-Z0-9._-]+" /></label>
        <label class="field"><span>{{ form.id ? '新密码（留空则不修改）' : '密码' }}</span><input v-model="form.password" type="password" autocomplete="new-password" :required="!form.id" minlength="12" /></label>
        <div class="field"><span>当前角色</span><strong class="assigned-role">{{ webRoleName(form.role) }}</strong></div>
        <label class="enabled-field"><input v-model="form.enabled" type="checkbox" /><span>启用账号</span></label>
        <p v-if="error" class="form-error" role="alert">{{ error }}</p>
      </form>
      <template #footer><button class="button secondary" :disabled="busy" @click="close">取消</button><button type="submit" form="web-user-form" class="button primary" :disabled="busy">{{ busy ? '保存中…' : form.id ? '保存修改' : '创建用户' }}</button></template>
    </Modal>
    <Modal v-if="pendingAction" :title="pendingAction.kind === 'delete' ? '删除用户' : '撤销访问令牌'" :busy="busy" @close="pendingAction = null">
      <p class="confirm-message">{{ pendingAction.kind === 'delete' ? `确定删除 ${pendingAction.user.username}？此操作无法撤销，其 MCP 令牌会立即失效。` : `确定撤销 ${pendingAction.user.username} 的 MCP 令牌？该用户需要重新生成令牌并更新客户端配置。` }}</p>
      <p v-if="error" class="form-error" role="alert">{{ error }}</p>
      <template #footer><button class="button secondary" :disabled="busy" @click="pendingAction = null">取消</button><button :class="['button', pendingAction.kind === 'delete' ? 'danger' : 'primary']" :disabled="busy" @click="confirmAction">{{ busy ? '处理中…' : pendingAction.kind === 'delete' ? '确认删除' : '确认撤销' }}</button></template>
    </Modal>
    <Modal v-if="tokenTarget" :title="issuedToken ? '个人令牌已签发' : `签发 ${tokenTarget.username} 的 MCP 令牌`" :busy="busy" @close="closeToken">
      <template v-if="issuedToken">
        <p class="confirm-message">令牌仅显示这一次。关闭后无法再次查看，请及时复制并交给 {{ tokenTarget.username }} 配置 MCP 客户端。</p>
        <label class="field token-field"><span>个人 MCP 令牌</span><input :value="issuedToken" readonly autocomplete="off" aria-label="新签发的个人 MCP 令牌" @focus="($event.target as HTMLInputElement).select()" /></label>
      </template>
      <p v-else class="confirm-message">{{ tokenTarget.hasToken ? '重新签发后，之前的令牌立即失效。' : '签发后，该用户可通过个人令牌访问 MCP 服务。' }}令牌只会显示一次。</p>
      <p v-if="error" class="form-error" role="alert">{{ error }}</p>
      <template #footer><button v-if="issuedToken" class="button secondary" :disabled="busy" @click="copyToken"><Icon name="copy" :size="15" />{{ tokenCopied ? '已复制' : '复制令牌' }}</button><button v-else class="button secondary" :disabled="busy" @click="closeToken">取消</button><button class="button primary" :disabled="busy" @click="issuedToken ? closeToken() : issueToken()">{{ busy ? '签发中…' : issuedToken ? '完成' : '确认签发' }}</button></template>
    </Modal>
  </section>
</template>

<style scoped>
.web-access{min-width:0;padding:0 0 28px}.access-toolbar{display:flex;align-items:center;gap:10px;margin:0 0 16px}.access-search{height:38px;width:min(300px,100%);display:flex;align-items:center;gap:8px;padding:0 11px;background:var(--surface);border:1px solid var(--border);border-radius:6px;color:var(--text-2)}.access-search:focus-within{border-color:var(--accent)}.access-search input{width:100%;min-width:0;border:0;outline:0;background:transparent;color:var(--text);font-size:12px;box-shadow:none}.access-count{margin-left:auto;color:var(--text-2);font-size:12px;white-space:nowrap}.access-table-wrap{min-width:0;overflow-x:auto;border-top:1px solid var(--border)}.access-table{width:100%;min-width:650px;border-collapse:collapse;text-align:left;font-size:12px}.access-table th{padding:13px 12px;background:var(--surface);color:var(--text-2);font-size:11px;font-weight:600;white-space:nowrap}.access-table td{padding:14px 12px;border-top:1px solid var(--border);color:var(--text-2)}.access-table tbody tr:hover{background:var(--surface-hover)}.access-table td strong{display:block;color:var(--text);font-size:13px;font-weight:600}.access-table td small{display:block;margin-top:4px;font-size:11px;color:var(--text-2)}.action-cell{text-align:right}.row-actions{display:flex;justify-content:flex-end;gap:12px;white-space:nowrap}.row-actions button{font-size:11px}.muted{color:var(--text-2)}.state-label{display:inline-flex;align-items:center;gap:7px;white-space:nowrap}.state-label i{width:6px;height:6px;border-radius:50%;background:var(--text-2)}.state-label.is-enabled i{background:var(--positive)}.empty-row{text-align:center;padding:42px 12px!important}.access-notice{color:var(--positive);font-size:12px;margin:0 0 12px}.user-form{display:grid;gap:16px}.user-form .field{min-width:0}.user-form input,.user-form select{color:var(--text);background:var(--surface);border-color:var(--border)}.user-form input{letter-spacing:0}.enabled-field{display:flex;align-items:center;gap:9px;font-size:12px;color:var(--text);cursor:pointer}.enabled-field input{width:16px;height:16px;accent-color:var(--accent)}.form-error{color:var(--danger);font-size:12px}
.access-table td .mobile-state{display:none}
.confirm-message{color:var(--text-2);font-size:13px;line-height:1.65}.confirm-message+.form-error{margin-top:12px}
.token-field{margin-top:16px;min-width:0}.token-field input{width:100%;font-family:Consolas,monospace;font-size:12px;letter-spacing:0;color:var(--text);background:var(--surface);border:1px solid var(--border);border-radius:6px;padding:10px}
.assigned-role{display:flex;align-items:center;min-height:38px;padding:0 11px;background:var(--surface-muted);border:1px solid var(--border);border-radius:6px;color:var(--text);font-size:12px;font-weight:600}
@media(max-width:1020px){.access-table th.action-cell,.access-table td.action-cell{position:sticky;right:0;min-width:126px;background:var(--surface);box-shadow:-5px 0 10px -8px #0008}.access-table tbody tr:hover td.action-cell{background:var(--surface-hover)}.row-actions{gap:7px}}
@media(max-width:720px){.access-toolbar{flex-wrap:wrap}.access-search{width:100%}.access-count{margin-left:0}.access-toolbar>.button.primary{margin-left:auto}.access-table{min-width:0;table-layout:fixed}.access-table th:nth-child(3),.access-table td:nth-child(3),.access-table th:nth-child(4),.access-table td:nth-child(4){display:none}.access-table th:first-child,.access-table td:first-child{width:45%}.access-table th:nth-child(2),.access-table td:nth-child(2){width:20%;padding-left:4px;padding-right:4px}.access-table th.action-cell,.access-table td.action-cell{position:static;min-width:0;width:35%;box-shadow:none;padding-left:4px;padding-right:8px}.access-table td strong{overflow:hidden;text-overflow:ellipsis}.row-actions{flex-wrap:wrap;gap:5px}.row-actions button{font-size:10px}.access-table td .mobile-state{display:block}}
</style>
