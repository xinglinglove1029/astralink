<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import type { AstraLinkController } from '../controller'
import { webManagement } from '../webBridge'
import Icon from './Icon.vue'

const props = defineProps<{ dock: AstraLinkController }>()
type Audit = { timestamp: number; actor: string; channel: string; action: string; outcome: string; profileId?: string; durationMs?: number }
type AuditPage = { items: Audit[]; total: number; page: number; pageSize: number }
const result = ref<AuditPage>({ items: [], total: 0, page: 1, pageSize: 20 })
const filters = ref({ actor: '', channel: '', action: '', outcome: '', profileId: '', from: '', to: '' })
const applied = ref<Record<string, unknown>>({})
const page = ref(1)
const pageSize = ref(20)
const loading = ref(false)
const error = ref('')
let requestId = 0
const pageCount = computed(() => Math.max(1, Math.ceil(result.value.total / pageSize.value)))
const profileName = (id?: string) => id ? props.dock.state.profiles.find(profile => profile.id === id)?.name ?? id : '—'

async function refresh() {
  const id = ++requestId
  loading.value = true
  error.value = ''
  try {
    const data = await webManagement<AuditPage>('list_web_audit', { ...applied.value, page: page.value, pageSize: pageSize.value })
    if (id === requestId) { result.value = data; page.value = data.page }
  } catch (cause) {
    if (id === requestId) error.value = cause instanceof Error ? cause.message : '读取失败'
  } finally { if (id === requestId) loading.value = false }
}
function search() {
  if (filters.value.from && filters.value.to && filters.value.from > filters.value.to) { error.value = '结束时间不能早于开始时间'; return }
  applied.value = Object.fromEntries(Object.entries(filters.value).filter(([, value]) => value).map(([key, value]) =>
    [key, key === 'from' ? new Date(value).getTime() : key === 'to' ? new Date(value).getTime() + 59_999 : value]))
  page.value = 1
  void refresh()
}
function clear() { filters.value = { actor: '', channel: '', action: '', outcome: '', profileId: '', from: '', to: '' }; search() }
function changePage(next: number) { page.value = next; void refresh() }
function changeSize() { page.value = 1; void refresh() }
onMounted(refresh)
</script>

<template>
  <section class="web-audit">
    <div class="audit-heading">
      <p>仅记录调用元数据，不保存命令、参数和输出。</p>
      <button class="button secondary small" :disabled="loading" @click="refresh"><Icon name="refresh" :size="15" />刷新</button>
    </div>
    <form class="audit-filters" @submit.prevent="search">
      <label>用户<input v-model.trim="filters.actor" maxlength="128" placeholder="用户名" /></label>
      <label>渠道<select v-model="filters.channel"><option value="">全部渠道</option><option value="web">网页</option><option value="mcp">MCP</option></select></label>
      <label>操作<input v-model.trim="filters.action" maxlength="128" placeholder="操作名称" /></label>
      <label>结果<select v-model="filters.outcome"><option value="">全部结果</option><option value="success">成功</option><option value="failure">失败</option><option value="denied">拒绝</option></select></label>
      <label>连接<select v-model="filters.profileId"><option value="">全部连接</option><option v-for="profile in props.dock.state.profiles" :key="profile.id" :value="profile.id">{{ profile.name }}</option></select></label>
      <label>开始时间<input v-model="filters.from" type="datetime-local" /></label>
      <label>结束时间<input v-model="filters.to" type="datetime-local" /></label>
      <div class="audit-filter-actions"><button class="button primary small" type="submit"><Icon name="search" :size="15" />筛选</button><button class="button secondary small" type="button" @click="clear">清除</button></div>
    </form>
    <p v-if="error" class="notice error" role="alert">{{ error }}</p>
    <div class="audit-scroll">
      <table>
        <thead><tr><th>时间</th><th>用户</th><th>渠道</th><th>操作</th><th>连接</th><th>结果</th><th>耗时</th></tr></thead>
        <tbody>
          <tr v-for="(event, index) in result.items" :key="`${event.timestamp}-${index}`"><td>{{ new Date(event.timestamp).toLocaleString() }}</td><td>{{ event.actor }}</td><td>{{ event.channel === 'mcp' ? 'MCP' : '网页' }}</td><td>{{ event.action }}</td><td :title="event.profileId">{{ profileName(event.profileId) }}</td><td>{{ event.outcome === 'success' ? '成功' : event.outcome === 'denied' ? '拒绝' : '失败' }}</td><td>{{ event.durationMs == null ? '—' : `${event.durationMs} ms` }}</td></tr>
          <tr v-if="!result.items.length"><td colspan="7">{{ loading ? '正在读取审计…' : '暂无匹配记录' }}</td></tr>
        </tbody>
      </table>
    </div>
    <div class="table-footer audit-pagination"><span>共 {{ result.total }} 条 · 当前 {{ result.total ? (page - 1) * pageSize + 1 : 0 }}–{{ Math.min(page * pageSize, result.total) }} 条</span><div class="pagination-controls"><label>每页 <select v-model.number="pageSize" aria-label="每页条数" @change="changeSize"><option :value="20">20</option><option :value="50">50</option><option :value="100">100</option></select> 条</label><button class="button secondary" :disabled="loading || page === 1" @click="changePage(page - 1)">上一页</button><span>第 {{ page }} / {{ pageCount }} 页</span><button class="button secondary" :disabled="loading || page === pageCount" @click="changePage(page + 1)">下一页</button></div></div>
  </section>
</template>

<style scoped>
.web-audit{min-width:0;padding:4px 0 32px}
.audit-heading{display:flex;justify-content:space-between;align-items:center;gap:16px;margin-bottom:16px}
.audit-heading p{color:var(--text-2);font-size:12px}
.audit-filters{display:flex;flex-wrap:wrap;align-items:end;gap:10px;margin-bottom:16px}
.audit-filters label{display:grid;gap:5px;color:var(--text-2);font-size:12px;min-width:130px;flex:1 1 145px}
.audit-filters input,.audit-filters select{width:100%;min-width:0;height:34px;border:1px solid var(--border);border-radius:6px;background:var(--surface);color:var(--text);padding:0 8px}
.audit-filter-actions{display:flex;gap:8px;align-items:center;height:34px}
.audit-scroll{max-height:min(65vh,700px);overflow:auto}
.audit-scroll table{width:100%;border-collapse:collapse;font-size:12px;text-align:left}
.audit-scroll th,.audit-scroll td{padding:12px 10px;border-bottom:1px solid var(--border);white-space:nowrap}
.audit-scroll th{position:sticky;top:0;background:var(--surface);color:var(--text-2);z-index:1}
.audit-pagination{display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;margin-top:14px;font-size:12px;color:var(--text-2)}
.audit-pagination .pagination-controls{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
@media(max-width:700px){.audit-heading{align-items:flex-start}.audit-heading p{max-width:240px}}
</style>
