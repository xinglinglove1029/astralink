<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { currentWebUser, webManagement, type WebUser } from '../webBridge'
import { webRoles, webRoleName, type WebRole } from '../webRoles'
import Icon from './Icon.vue'
import Modal from './Modal.vue'

const users = ref<WebUser[]>([])
const editing = ref<WebRole | null>(null)
const assignments = reactive<Record<string, WebRole | null>>({})
const availableSearch = ref('')
const memberSearch = ref('')
const availableSelected = ref<string[]>([])
const memberSelected = ref<string[]>([])
const confirming = ref(false)
const loading = ref(false)
const busy = ref(false)
const notice = ref('')
const error = ref('')
const selected = computed(() => webRoles.find(role => role.id === editing.value))
const available = computed(() => users.value.filter(user => user.id !== 'admin' && assignments[user.id] !== editing.value))
const members = computed(() => users.value.filter(user => assignments[user.id] === editing.value))
const visibleAvailable = computed(() => available.value.filter(user => user.username.toLocaleLowerCase().includes(availableSearch.value.trim().toLocaleLowerCase())))
const visibleMembers = computed(() => members.value.filter(user => user.username.toLocaleLowerCase().includes(memberSearch.value.trim().toLocaleLowerCase())))
const selectableMembers = computed(() => visibleMembers.value.filter(user => user.id !== 'admin'))
const changes = computed(() => users.value.filter(user => user.id !== 'admin' && assignments[user.id] !== user.role))
const allAvailableSelected = computed(() => visibleAvailable.value.length > 0 && visibleAvailable.value.every(user => availableSelected.value.includes(user.id)))
const allMembersSelected = computed(() => selectableMembers.value.length > 0 && selectableMembers.value.every(user => memberSelected.value.includes(user.id)))
watch(availableSearch, () => { availableSelected.value = [] })
watch(memberSearch, () => { memberSelected.value = [] })

async function refresh() {
  if (currentWebUser.value?.role !== 'admin') return
  loading.value = true
  error.value = ''
  try { users.value = await webManagement<WebUser[]>('list_web_users') }
  catch (cause) { error.value = cause instanceof Error ? cause.message : '读取角色成员失败' }
  finally { loading.value = false }
}
function openRole(role: WebRole) {
  editing.value = role
  availableSearch.value = ''
  memberSearch.value = ''
  availableSelected.value = []
  memberSelected.value = []
  error.value = ''
  for (const user of users.value) assignments[user.id] = user.role
}
function close() { if (!busy.value && !confirming.value) editing.value = null }
function selectAll(side: 'available' | 'members') {
  const ids = (side === 'available' ? visibleAvailable.value : selectableMembers.value).map(user => user.id)
  const selectedIds = side === 'available' ? availableSelected : memberSelected
  selectedIds.value = ids.every(id => selectedIds.value.includes(id))
    ? selectedIds.value.filter(id => !ids.includes(id))
    : [...new Set([...selectedIds.value, ...ids])]
}
function assign() {
  if (!editing.value) return
  for (const id of availableSelected.value) assignments[id] = editing.value
  availableSelected.value = []
  memberSelected.value = []
}
function remove() {
  if (busy.value || !editing.value || !memberSelected.value.length) return
  for (const id of memberSelected.value) if (id !== 'admin') assignments[id] = null
  availableSelected.value = []
  memberSelected.value = []
}
async function save() {
  const pending = changes.value.map(user => ({ user, role: assignments[user.id] }))
    .sort((a, b) => Number(a.user.id === currentWebUser.value?.id) - Number(b.user.id === currentWebUser.value?.id))
  if (busy.value || !pending.length) return
  busy.value = true
  error.value = ''
  let saved = 0
  try {
    for (const { user, role } of pending) {
      await webManagement('assign_web_user_role', { id: user.id, role })
      saved++
    }
    confirming.value = false
    editing.value = null
    notice.value = `已更新 ${saved} 位用户的角色`
    if (pending.some(({ user }) => user.id === currentWebUser.value?.id)) {
      window.dispatchEvent(new Event('astralink-session-expired'))
      return
    }
    await refresh()
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : '保存失败'
    confirming.value = false
    await refresh()
    for (const user of users.value) assignments[user.id] = user.role
    availableSelected.value = []
    memberSelected.value = []
    error.value = saved ? `已更新 ${saved} 位用户；其余未完成：${message}` : message
  } finally { busy.value = false }
}
onMounted(refresh)
</script>

<template>
  <section class="web-roles">
    <div class="roles-toolbar"><span>权限固定的内置角色，调整成员后立即生效。</span><button class="button secondary small" :disabled="loading || busy" aria-label="刷新角色列表" title="刷新角色列表" @click="refresh"><Icon name="refresh" :size="15" /></button></div>
    <p v-if="notice" class="role-notice" role="status">{{ notice }}</p>
    <p v-if="error && !editing" class="notice error" role="alert">{{ error }}</p>
    <div class="role-table-wrap">
      <table class="role-table">
        <thead><tr><th scope="col">角色</th><th scope="col">权限范围</th><th scope="col">成员数</th><th scope="col">类型</th><th scope="col" class="action-cell">操作</th></tr></thead>
        <tbody><tr v-for="role in webRoles" :key="role.id">
          <td><strong>{{ role.name }}</strong><small class="role-key">{{ role.id }}</small></td>
          <td class="permission-cell">{{ role.permissions.join(' · ') }}</td>
          <td><span class="member-count">{{ users.filter(user => user.role === role.id).length }}</span> 人</td>
          <td>内置</td>
          <td class="action-cell"><button class="text-button" :disabled="loading || busy" :aria-label="`管理${role.name}成员`" @click="openRole(role.id)">管理成员</button></td>
        </tr></tbody>
      </table>
    </div>

    <Modal v-if="editing && selected" :title="`管理${selected.name}成员`" subtitle="先移动用户，再确认保存角色变更。" wide :busy="busy || confirming" :inactive="confirming" @close="close">
      <div class="role-editor">
        <div class="role-scope"><strong>权限范围</strong><span>{{ selected.permissions.join(' · ') }}</span></div>
        <div class="transfer">
          <section class="transfer-side" aria-label="待分配用户">
            <div class="transfer-heading"><strong>待分配用户</strong><span>{{ available.length }}</span></div>
            <label class="transfer-search"><Icon name="search" :size="14" /><span class="sr-only">搜索待分配用户</span><input v-model="availableSearch" type="search" placeholder="搜索用户" /></label>
            <label class="select-all"><input type="checkbox" :checked="allAvailableSelected" :disabled="!visibleAvailable.length || busy" @change="selectAll('available')" />全选当前结果</label>
            <div class="transfer-list"><label v-for="user in visibleAvailable" :key="user.id" class="transfer-item"><input v-model="availableSelected" type="checkbox" :value="user.id" :disabled="busy" /><span class="transfer-name">{{ user.username }}<small v-if="!user.enabled">已停用</small></span><span class="transfer-role">{{ webRoleName(assignments[user.id] ?? null) }}</span></label><p v-if="!visibleAvailable.length" class="transfer-empty">暂无可分配用户</p></div>
          </section>
          <div class="transfer-controls">
            <button class="transfer-arrow assign-arrow" type="button" :disabled="!availableSelected.length || busy" :aria-label="`分配选中用户到${selected.name}`" :title="`分配到${selected.name}`" @click="assign"><Icon name="arrow" :size="17" /></button>
            <button class="transfer-arrow remove-arrow" type="button" :disabled="!memberSelected.length || busy" aria-label="移出选中成员到待分配用户" title="移出成员" @click="remove"><Icon name="arrow" :size="17" /></button>
          </div>
          <section class="transfer-side" :aria-label="`${selected.name}成员`">
            <div class="transfer-heading"><strong>{{ selected.name }}成员</strong><span>{{ members.length }}</span></div>
            <label class="transfer-search"><Icon name="search" :size="14" /><span class="sr-only">搜索当前成员</span><input v-model="memberSearch" type="search" placeholder="搜索成员" /></label>
            <label class="select-all"><input type="checkbox" :checked="allMembersSelected" :disabled="!selectableMembers.length || busy" @change="selectAll('members')" />全选可调整成员</label>
            <div class="transfer-list"><label v-for="user in visibleMembers" :key="user.id" class="transfer-item" :class="{ locked: user.id === 'admin' }"><input v-if="user.id !== 'admin'" v-model="memberSelected" type="checkbox" :value="user.id" :disabled="busy" /><span v-else class="checkbox-spacer" /><span class="transfer-name">{{ user.username }}<small v-if="!user.enabled">已停用</small></span><span v-if="user.id === 'admin'" class="transfer-role">固定</span></label><p v-if="!visibleMembers.length" class="transfer-empty">暂无成员</p></div>
          </section>
        </div>
        <p v-if="error" class="form-error" role="alert">{{ error }}</p>
      </div>
      <template #footer><span class="pending-count">{{ changes.length ? `${changes.length} 项待保存` : '没有待保存的更改' }}</span><button class="button secondary" :disabled="busy" @click="close">取消</button><button class="button primary" :disabled="busy || !changes.length" @click="confirming = true">保存变更</button></template>
    </Modal>

    <Modal v-if="confirming" title="确认角色变更" :subtitle="`${changes.length} 位用户的角色将更新`" :busy="busy" @close="confirming = false">
      <p class="confirm-message">保存后，旧会话和个人 MCP 令牌将立即失效；未分配角色的用户无法登录 Web 或使用 MCP。</p>
      <template #footer><button class="button secondary" :disabled="busy" @click="confirming = false">返回检查</button><button class="button primary" :disabled="busy" @click="save">{{ busy ? '保存中…' : '确认保存' }}</button></template>
    </Modal>
  </section>
</template>

<style scoped>
.web-roles{min-width:0;padding:0 0 28px}
.roles-toolbar{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:16px;color:var(--text-2);font-size:12px}
.role-table-wrap{min-width:0;overflow-x:auto;border-top:1px solid var(--border)}
.role-table{width:100%;min-width:680px;border-collapse:collapse;text-align:left;font-size:12px}
.role-table th{padding:13px 12px;background:var(--surface);color:var(--text-2);font-size:11px;font-weight:600;white-space:nowrap}
.role-table td{padding:17px 12px;border-top:1px solid var(--border);color:var(--text-2)}
.role-table tbody tr:hover{background:var(--surface-hover)}
.role-table td strong{display:block;font-size:13px;color:var(--text);font-weight:600}
.role-key{display:block;margin-top:4px;color:var(--text-2);font-size:11px}
.permission-cell{max-width:320px;line-height:1.6}.member-count{font-weight:650;color:var(--text)}
.action-cell{text-align:right;white-space:nowrap}.action-cell .text-button{font-size:11px}
.role-notice{font-size:12px;color:var(--positive);margin-bottom:12px}
.role-editor{display:grid;gap:14px}
.role-scope{display:grid;gap:6px;padding:10px 12px;background:var(--surface-muted);border:1px solid var(--border);border-radius:6px;font-size:12px;color:var(--text-2)}
.role-scope strong{font-size:11px;color:var(--text)}
.transfer{display:grid;grid-template-columns:minmax(0,1fr) 42px minmax(0,1fr);align-items:center;gap:8px}
.transfer-side{min-width:0;border:1px solid var(--border);border-radius:6px;overflow:hidden;background:var(--surface)}
.transfer-heading{height:36px;padding:0 12px;display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid var(--border);color:var(--text);font-size:12px}
.transfer-heading span{color:var(--text-2);font-size:11px}
.transfer-search{height:36px;display:flex;align-items:center;gap:7px;margin:8px 10px;border:1px solid var(--border);border-radius:5px;padding:0 8px;color:var(--text-2)}
.transfer-search input{width:100%;min-width:0;background:transparent;border:0;outline:0;box-shadow:none;color:var(--text);font-size:11px}
.select-all{display:flex;align-items:center;gap:7px;border-bottom:1px solid var(--border);padding:4px 12px 9px;font-size:11px;color:var(--text-2);cursor:pointer}
.select-all input,.transfer-item input{width:15px;height:15px;accent-color:var(--accent);flex:none}
.transfer-list{height:205px;overflow:auto}.transfer-item{min-height:40px;display:flex;align-items:center;gap:8px;padding:7px 12px;border-bottom:1px solid var(--border);font-size:12px;color:var(--text);cursor:pointer}
.transfer-item:hover{background:var(--surface-hover)}.transfer-item.locked{cursor:default}.transfer-name{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.transfer-name small{margin-left:5px;color:var(--text-2);font-size:10px}.transfer-role{margin-left:auto;color:var(--text-2);font-size:10px;white-space:nowrap}.checkbox-spacer{width:15px;flex:none}
.transfer-empty{padding:24px 10px;color:var(--text-2);font-size:11px;text-align:center}
.transfer-controls{display:flex;flex-direction:column;align-items:center;gap:10px}
.transfer-arrow{height:34px;width:34px;display:grid;place-items:center;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--accent);cursor:pointer}
.transfer-arrow:hover:not(:disabled){background:var(--accent-soft);border-color:var(--accent)}.remove-arrow svg{transform:rotate(180deg)}
.pending-count{margin-right:auto;color:var(--text-2);font-size:11px}.form-error{color:var(--danger);font-size:12px}
.confirm-message{font-size:13px;line-height:1.65;color:var(--text-2)}
@media(max-width:1020px){.role-table th.action-cell,.role-table td.action-cell{position:sticky;right:0;min-width:95px;background:var(--surface);box-shadow:-5px 0 10px -8px #0008}.role-table tbody tr:hover td.action-cell{background:var(--surface-hover)}}
@media(max-width:720px){.roles-toolbar{align-items:flex-start}.pending-count{display:none}.role-table{min-width:0;table-layout:fixed}.role-table th:nth-child(2),.role-table td:nth-child(2),.role-table th:nth-child(4),.role-table td:nth-child(4){display:none}.role-table th:first-child,.role-table td:first-child{width:38%}.role-table th:nth-child(3),.role-table td:nth-child(3){width:25%}.role-table th.action-cell,.role-table td.action-cell{position:static;min-width:0;width:37%;box-shadow:none}.transfer{grid-template-columns:minmax(0,1fr);gap:8px}.transfer-list{height:96px}.transfer-controls{flex-direction:row;justify-content:center}.assign-arrow svg{transform:rotate(90deg)}.remove-arrow svg{transform:rotate(-90deg)}.transfer-item{min-height:36px}}
</style>
