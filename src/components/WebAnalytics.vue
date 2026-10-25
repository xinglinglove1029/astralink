<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { webManagement } from '../webBridge'
import type { AstraLinkController } from '../controller'
import Icon from './Icon.vue'
const props = defineProps<{dock:AstraLinkController}>()
const report = ref<{ total:number; failures:number; windowDays:number; byDay:Record<string,number>; byAction:Record<string,number>; byActor:Record<string,number>; actorNames:Record<string,string>; byChannel:Record<string,number>; byProfile:Record<string,number> } | null>(null)
const loading = ref(false)
const error = ref('')
const maxDay = computed(() => Math.max(1, ...Object.values(report.value?.byDay ?? {})))
const maxAction = computed(() => Math.max(1, ...Object.values(report.value?.byAction ?? {})))
const maxActor = computed(() => Math.max(1, ...Object.values(report.value?.byActor ?? {})))
const maxProfile = computed(() => Math.max(1, ...Object.values(report.value?.byProfile ?? {})))
const profileName = (id:string) => props.dock.state.profiles.find(profile => profile.id === id)?.name ?? '已删除连接'
const groups = computed(() => Object.entries(props.dock.state.profiles.reduce((counts, profile) => { const name=profile.group||'未分组';counts[name]=(counts[name]??0)+1;return counts }, {} as Record<string,number>)))
const sources = computed(() => Object.entries(props.dock.state.profiles.reduce((counts, profile) => { counts[profile.source]=(counts[profile.source]??0)+1;return counts }, {} as Record<string,number>)))
const days = computed(() => Array.from({length:7}, (_,i) => { const epochDay=Math.floor(Date.now()/86_400_000)-6+i;return {key:String(epochDay),label:new Date(epochDay*86_400_000).toLocaleDateString(undefined,{timeZone:'UTC',month:'numeric',day:'numeric'}),count:report.value?.byDay[String(epochDay)]??0} }))
async function load() { loading.value = true; error.value = ''; try { report.value = await webManagement('web_report') } catch (e) { error.value = e instanceof Error ? e.message : '统计读取失败' } finally { loading.value = false } }
onMounted(load)
</script>
<template>
  <section class="analytics-page">
    <div class="analytics-head"><p>按用户、操作和时间查看最近 7 天调用情况。</p><button class="button secondary" :disabled="loading" @click="load"><Icon name="refresh" :size="15"/>刷新</button></div>
    <p v-if="error" class="notice error">{{ error }}</p>
    <div v-if="report" class="analytics-grid">
      <article class="metric-card"><span>近 {{ report.windowDays }} 天调用</span><strong>{{ report.total }}</strong><small>已完成的网页和 MCP 操作</small></article>
      <article class="metric-card"><span>失败或拒绝</span><strong>{{ report.failures }}</strong><small>{{ report.total ? Math.round(report.failures / report.total * 100) : 0 }}% 占比</small></article>
      <article class="metric-card"><span>活跃用户</span><strong>{{ Object.keys(report.byActor).length }}</strong><small>有审计记录的用户</small></article>
      <section class="chart-card"><div class="chart-title"><h2>按用户</h2><span>调用次数</span></div><div v-for="(count, actor) in report.byActor" :key="actor" class="bar-row"><label :title="actor">{{ report.actorNames[actor] ?? actor }}</label><div class="bar"><i :style="{width:`${count / maxActor * 100}%`}"/></div><b>{{ count }}</b></div><p v-if="!Object.keys(report.byActor).length" class="empty">暂无调用记录</p></section>
      <section class="chart-card"><div class="chart-title"><h2>按操作</h2><span>网页 / MCP</span></div><div v-for="(count, action) in report.byAction" :key="action" class="bar-row"><label>{{ action }}</label><div class="bar teal"><i :style="{width:`${count / maxAction * 100}%`}"/></div><b>{{ count }}</b></div><p v-if="!Object.keys(report.byAction).length" class="empty">暂无调用记录</p></section>
      <section class="chart-card"><div class="chart-title"><h2>按渠道</h2><span>调用次数</span></div><div v-for="(count, channel) in report.byChannel" :key="channel" class="bar-row"><label>{{ channel === 'mcp' ? 'MCP' : '网页' }}</label><div class="bar"><i :style="{width:`${count / Math.max(1,report.total)*100}%`}"/></div><b>{{ count }}</b></div><p v-if="!Object.keys(report.byChannel).length" class="empty">暂无调用记录</p></section>
      <section class="chart-card"><div class="chart-title"><h2>按连接</h2><span>调用次数</span></div><div v-for="(count, id) in report.byProfile" :key="id" class="bar-row"><label :title="id">{{ profileName(id) }}</label><div class="bar teal"><i :style="{width:`${count/maxProfile*100}%`}"/></div><b>{{ count }}</b></div><p v-if="!Object.keys(report.byProfile).length" class="empty">暂无连接调用</p></section>
      <section class="chart-card"><div class="chart-title"><h2>连接分组</h2><span>当前配置</span></div><div v-for="[group,count] in groups" :key="group" class="bar-row"><label>{{ group }}</label><div class="bar"><i :style="{width:`${count/Math.max(1,props.dock.state.profiles.length)*100}%`}"/></div><b>{{ count }}</b></div></section>
      <section class="chart-card"><div class="chart-title"><h2>导入来源</h2><span>当前配置</span></div><div v-for="[source,count] in sources" :key="source" class="bar-row"><label>{{ source }}</label><div class="bar teal"><i :style="{width:`${count/Math.max(1,props.dock.state.profiles.length)*100}%`}"/></div><b>{{ count }}</b></div></section>
      <section class="chart-card wide"><div class="chart-title"><h2>近 7 天趋势</h2><span>UTC 日期 · 已保留记录</span></div><div class="day-chart"><div v-for="day in days" :key="day.key" class="day"><i :style="{height:`${day.count/maxDay*100}%`}"/><label>{{ day.label }}</label></div></div><p class="empty">网页审计保留 90 天；MCP 审计保留最近 16 MiB。历史超过保留范围时统计不包含已轮换记录。</p></section>
    </div>
    <p v-else-if="loading" class="notice info">正在加载统计…</p>
  </section>
</template>
<style scoped>
.analytics-page{padding:4px 0 32px}.analytics-head{display:flex;justify-content:space-between;align-items:flex-end;gap:16px;margin-bottom:24px}.analytics-head h1{font-size:30px;color:var(--text);margin-top:8px}.analytics-head p{color:var(--text-2);font-size:12px;margin-top:6px}.analytics-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px}.metric-card,.chart-card{border:1px solid var(--border);background:var(--surface);border-radius:8px;padding:18px}.metric-card span,.metric-card small,.chart-title span{color:var(--text-2);font-size:11px}.metric-card strong{display:block;color:var(--accent);font-size:31px;margin:9px 0 4px}.chart-card{grid-column:span 1;min-width:0}.chart-card.wide{grid-column:1/-1}.chart-title{display:flex;justify-content:space-between;align-items:center;margin-bottom:16px}.chart-title h2{font-size:14px;color:var(--text)}.bar-row{display:grid;grid-template-columns:minmax(80px,130px) 1fr 32px;align-items:center;gap:10px;margin:13px 0;font-size:11px}.bar-row label{color:var(--text-2);overflow:hidden;text-overflow:ellipsis}.bar{height:8px;background:var(--surface-muted);border-radius:5px;overflow:hidden}.bar i{display:block;height:100%;background:var(--accent);border-radius:inherit}.bar.teal i{background:#42b8aa}.bar-row b{color:var(--text);text-align:right}.day-chart{height:180px;display:flex;align-items:end;gap:12px;padding:10px 8px 0;border-bottom:1px solid var(--border)}.day{height:100%;flex:1;display:flex;flex-direction:column;justify-content:end;align-items:center;gap:7px;color:var(--text-2);font-size:10px}.day i{width:min(32px,70%);min-height:2px;background:var(--accent);border-radius:4px 4px 0 0}.empty{color:var(--text-2);font-size:11px;padding:20px 0}@media(max-width:760px){.analytics-head{align-items:flex-start}.analytics-grid{grid-template-columns:1fr}.chart-card,.chart-card.wide{grid-column:auto}.analytics-head h1{font-size:24px}}
</style>
