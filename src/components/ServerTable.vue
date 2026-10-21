<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import Icon from './Icon.vue'
import { authLabels, sourceLabels, needsCredential, connectionResultLabel } from '../domain'
import { paginate, pageSelection } from '../pagination'
import type { ConnectionProfile, ConnectionTest, SourceKind } from '../types'
const props = defineProps<{ profiles: ConnectionProfile[]; selectedId: string; results: Record<string, ConnectionTest>; testing: Record<string, boolean>; desktop: boolean; ready: boolean; running: boolean; busy: boolean; filtered: boolean; filterKey: string }>()
const emit = defineEmits<{ select: [id: string]; edit: [profile: ConnectionProfile]; test: [id: string]; delete: [profile: ConnectionProfile]; deleteBatch: [ids: string[]]; create: []; import: []; reset: [] }>()
const checkedIds = ref<string[]>([])
const page = ref(1)
const pageSize = ref(20)
const pagination = computed(() => paginate(props.profiles,page.value,pageSize.value))
const visibleIds = computed(() => pagination.value.items.map(profile=>profile.id))
const allChecked = computed(() => pagination.value.items.length > 0 && checkedIds.value.length === pagination.value.items.length)
const someChecked = computed(() => checkedIds.value.length > 0 && !allChecked.value)
const canDelete = computed(() => props.desktop && props.ready && !props.busy)
watch(() => props.filterKey, () => { page.value=1; checkedIds.value=[] })
watch(pageSize, () => { page.value=1; checkedIds.value=[] })
watch(visibleIds, ids => {
  page.value=pagination.value.page
  checkedIds.value=pageSelection(checkedIds.value,ids)
  if(ids.length && !ids.includes(props.selectedId)) emit('select',ids[0]!)
})
watch(() => props.selectedId, id => {
  const index=props.profiles.findIndex(profile=>profile.id===id)
  if(index>=0) page.value=Math.floor(index/pageSize.value)+1
})
function toggleAll() { checkedIds.value = allChecked.value ? [] : [...visibleIds.value] }
function sourceName(source: string) { return source === 'manual' ? '手动添加' : sourceLabels[source as SourceKind] || source }
</script>
<template>
  <div class="server-list card">
    <div class="list-caption"><span>服务器列表 <b>{{ profiles.length }}</b></span><span v-if="checkedIds.length" class="batch-actions">当前页已选 {{ checkedIds.length }} 台<button class="text-button" @click="checkedIds = []">取消选择</button><button class="text-button danger-text" :disabled="!canDelete" @click="emit('deleteBatch', [...checkedIds])"><Icon name="trash" :size="14" />批量删除</button></span><span v-else>点击行查看详情</span></div>
    <div v-if="profiles.length" class="table-scroll">
      <table class="server-table">
        <thead><tr><th class="selection-cell"><input type="checkbox" aria-label="全选当前页" :checked="allChecked" :indeterminate="someChecked" :disabled="!canDelete" @change="toggleAll" /></th><th>服务器 / 地址</th><th>所属分组</th><th>账号</th><th>认证</th><th>状态</th><th class="actions-cell">操作</th></tr></thead>
        <tbody>
          <tr v-for="profile in pagination.items" :key="profile.id" :class="{ 'selected-row': profile.id === selectedId }" @click="emit('select', profile.id)">
            <td class="selection-cell" @click.stop><input v-model="checkedIds" type="checkbox" :value="profile.id" :aria-label="'选择 ' + profile.name" :disabled="!canDelete" /></td>
            <td><button class="server-name" @click.stop="emit('select', profile.id)"><span class="server-avatar"><Icon name="server" :size="19" /></span><span><strong>{{ profile.name }}</strong><span class="address mono">{{ profile.host }}:{{ profile.port }}</span><span class="row-meta">{{ sourceName(profile.source) }}</span></span></button></td>
            <td><span class="table-group" :title="profile.group || '未分组'">{{ profile.group || '未分组' }}</span></td>
            <td><span class="mono table-user">{{ profile.username }}</span></td>
            <td><span class="auth-label"><Icon :name="profile.authType === 'privateKey' ? 'key' : 'shield'" :size="14" />{{ authLabels[profile.authType] }}</span></td>
            <td>
              <span v-if="!profile.enabled" class="badge neutral">已停用</span>
              <span v-else-if="testing[profile.id]" class="badge blue"><span class="spinner" />测试中</span>
              <span v-else-if="results[profile.id]" :class="['badge', results[profile.id]!.status === 'connected' ? 'green' : results[profile.id]!.status === 'failed' ? 'red' : 'amber']">{{ connectionResultLabel(results[profile.id]!.status) }}</span>
              <span v-else-if="needsCredential(profile)" class="badge amber">待补凭据</span>
              <span v-else class="badge neutral">未测试</span>
            </td>
            <td class="actions-cell" @click.stop><button class="text-button" :disabled="!desktop || !ready || !running || busy || !profile.enabled || testing[profile.id]" @click="emit('test', profile.id)">测试</button><button class="icon-button" :aria-label="`编辑 ${profile.name}`" :disabled="!desktop || !ready || busy" @click="emit('edit', profile)"><Icon name="edit" :size="16" /></button><button class="text-button danger-text" :aria-label="'删除 ' + profile.name" :disabled="!canDelete" @click="emit('delete', profile)"><Icon name="trash" :size="15" />删除</button></td>
          </tr>
        </tbody>
      </table>
    </div>
    <div v-else class="empty-state">
      <span class="empty-icon"><Icon :name="filtered ? 'search' : 'server'" :size="30" /></span>
      <h3>{{ filtered ? '没有匹配的服务器' : '从第一台服务器开始' }}</h3>
      <p>{{ filtered ? '试试其他关键词，或清除筛选条件。' : '导入已有连接，或手动添加服务器，统一管理 SSH 与 MCP。' }}</p>
      <div v-if="filtered"><button class="button secondary" @click="emit('reset')">清除筛选</button></div>
      <div v-else class="button-row"><button class="button primary" :disabled="!desktop || !ready || busy" @click="emit('create')"><Icon name="plus" :size="16" />添加服务器</button><button class="button secondary" @click="emit('import')"><Icon name="download" :size="16" />导入已有连接</button></div>
    </div>
    <div class="table-footer server-pagination"><span>共 {{ profiles.length }} 台 · 当前 {{ pagination.from }}–{{ pagination.to }} 台</span><div class="pagination-controls"><label>每页 <select v-model.number="pageSize" aria-label="每页条数"><option :value="20">20</option><option :value="50">50</option><option :value="100">100</option></select> 条</label><button class="button secondary" :disabled="pagination.page===1" @click="page=pagination.page-1" aria-label="上一页">上一页</button><span>第 {{ pagination.page }} / {{ pagination.pageCount }} 页</span><button class="button secondary" :disabled="pagination.page===pagination.pageCount" @click="page=pagination.page+1" aria-label="下一页">下一页</button></div></div>
  </div>
</template>

<style scoped>
.server-table { min-width: 790px; }
.server-table tbody tr { cursor: pointer; }
.selection-cell { width: 36px; text-align: center; }
.selection-cell input { width: 15px; height: 15px; accent-color: #3269ed; cursor: pointer; }
.table-group { display: block; max-width: 135px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; color: #53647c; }
.server-table .actions-cell { position: sticky; right: 0; min-width: 128px; background: #fff; box-shadow: -6px 0 10px -10px #66758c; }
.server-table th.actions-cell { background: #f9fbfe; }
.server-table tr.selected-row .actions-cell { background: #f7faff; }
.server-table tbody tr:hover .actions-cell { background: #fafcff; }
.actions-cell .danger-text { display: inline-flex; align-items: center; gap: 4px; }
.batch-actions { display: inline-flex; align-items: center; gap: 12px; }
</style>
