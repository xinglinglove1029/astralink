<script setup lang="ts">
import { computed,onMounted,onUnmounted,ref,watch } from 'vue'
import type { AstraLinkController } from '../controller'
import { buildConstellation,dashboardSummary } from '../dashboard'
import { connectionResultLabel,needsCredential } from '../domain'
import Icon from './Icon.vue'
import NexusMark from './NexusMark.vue'
import { webMode } from '../webBridge'
const props=defineProps<{dock:AstraLinkController}>()
const emit=defineEmits<{navigate:[page:string];openServer:[id:string];create:[]}>()
const {state}=props.dock
const summary=computed(()=>dashboardSummary(state.profiles,state.results))
const graph=computed(()=>buildConstellation(state.profiles))
const selectedId=ref(state.profiles[0]?.id??'')
const selected=computed(()=>state.profiles.find(p=>p.id===selectedId.value))
const configuredClients=computed(()=>Object.keys(state.clientRegistrations).length)
const recent=computed(()=>state.runtimeLogs.entries.slice(-5).reverse())
const ready=computed(()=>state.desktop&&state.ready)
const groups=computed(()=>{
  const map=new Map<string,number>();for(const p of state.profiles)map.set(p.group||'未分组',(map.get(p.group||'未分组')??0)+1)
  return [...map].sort((a,b)=>b[1]-a[1]).slice(0,4)
})
const stars=Array.from({length:36},(_,i)=>({x:(i*137.5)%100,y:(i*61.7)%100,delay:-(i%7),size:i%4===0?2:1}))
function path(node:typeof graph.value.nodes[number]) {
  const parent=graph.value.nodes.find(p=>p.id===node.parentId)
  const x=parent?.x??450,y=parent?.y??280
  return `M ${x} ${y} Q ${(x+node.x)/2+30} ${(y+node.y)/2-22} ${node.x} ${node.y}`
}
function pick(id:string){ selectedId.value=id }
function nodeStatus(id:string){const p=state.profiles.find(p=>p.id===id);return !p?.enabled?'已停用':state.results[id]?connectionResultLabel(state.results[id]!.status):'已配置 · 未测试'}
watch(()=>graph.value.nodes, nodes=>{if(!nodes.some(n=>n.id===selectedId.value))selectedId.value=nodes[0]?.id??''})
let timer:ReturnType<typeof setInterval>|undefined
function refresh(){if(ready.value&&typeof document!=='undefined'&&!document.hidden)void props.dock.refreshRuntimeLogs()}
watch(()=>state.ready,refresh)
onMounted(()=>{refresh();timer=setInterval(refresh,3000)})
onUnmounted(()=>{if(timer)clearInterval(timer)})
</script>
<template>
  <div class="nexus-dashboard">
    <header class="dashboard-welcome"><div><span class="section-kicker"><span class="tiny-star"/> YOUR CONNECTED UNIVERSE</span><h1>万物相连，<span>由此开始。</span></h1><p><b>AstraLink</b><span class="welcome-divider">/</span>把散落的节点，连接成你的工作宇宙。</p></div><div class="dashboard-actions"><button class="button secondary" @click="emit('navigate','import')"><Icon name="download" :size="15"/>导入连接 <small>Import</small></button><button class="button primary" :disabled="!ready||!!state.mutation" @click="emit('create')"><Icon name="plus" :size="15"/>添加节点 <small>New node</small></button></div></header>
    <div class="dashboard-metrics">
      <button class="metric-cell" @click="emit('navigate','servers')"><span class="metric-caption">连接节点 <small>NODES</small></span><div><strong>{{ summary.total.toString().padStart(2,'0') }}</strong><Icon name="server" :size="19"/></div><p>{{ summary.groups }} 个分组 <span>查看全部 <Icon name="arrow" :size="12"/></span></p></button>
      <button class="metric-cell" @click="emit('navigate','servers')"><span class="metric-caption">已启用 <small>ENABLED</small></span><div><strong>{{ summary.enabled.toString().padStart(2,'0') }}</strong><span class="metric-orbit"/></div><p>{{ state.profiles.filter(needsCredential).length }} 台待补凭据 <span>配置状态</span></p></button>
      <button class="metric-cell" @click="emit('navigate','logs')"><span class="metric-caption">测试通过 <small>VERIFIED</small></span><div><strong>{{ summary.passed.toString().padStart(2,'0') }}</strong><Icon name="shield" :size="20"/></div><p>本次会话测试 {{ summary.tested }} 台 <span>查看日志</span></p></button>
      <button class="metric-cell" @click="emit('navigate','settings')"><span class="metric-caption">{{ webMode ? '配置迁移' : '客户端接入' }} <small>{{ webMode ? 'BACKUP' : 'CLIENTS' }}</small></span><div><strong>{{ webMode ? '--' : configuredClients.toString().padStart(2,'0') }}</strong><Icon name="link" :size="20"/></div><p>{{ webMode ? '管理加密备份' : '本次会话已配置' }} <span>{{ webMode ? '查看设置' : 'MCP' }}</span></p></button>
    </div>
    <div class="dashboard-main-grid">
      <section class="constellation-card">
        <div class="constellation-heading"><div><span class="section-kicker">CONSTELLATION</span><h2>连接星图 <small>Everything, connected.</small></h2></div><span class="map-indicator"><i/>{{ webMode ? '服务器连接拓扑' : '本机连接拓扑' }}</span></div>
        <div class="constellation-stage" :class="{'is-empty':!graph.nodes.length}">
          <span v-for="(star,i) in stars" :key="i" class="space-star" :style="{left:star.x+'%',top:star.y+'%',width:star.size+'px',height:star.size+'px',animationDelay:star.delay+'s'}"/>
          <div class="map-grain"/>
          <svg class="constellation-map" viewBox="0 0 900 560" role="group" aria-label="连接星图 / Configured connection topology">
            <defs><radialGradient id="coreHalo"><stop offset="0" stop-color="var(--accent)" stop-opacity=".18"/><stop offset="1" stop-color="var(--accent)" stop-opacity="0"/></radialGradient><linearGradient id="linkGlow"><stop stop-color="var(--accent)" stop-opacity=".6"/><stop offset="1" stop-color="var(--accent-2)" stop-opacity=".12"/></linearGradient></defs>
            <circle cx="450" cy="280" r="165" fill="url(#coreHalo)"/>
            <g class="orbit-guides"><ellipse cx="450" cy="280" rx="240" ry="150"/><ellipse cx="450" cy="280" rx="355" ry="212"/><ellipse cx="450" cy="280" rx="310" ry="90" transform="rotate(-27 450 280)"/></g>
            <g v-for="node in graph.nodes" :key="`edge-${node.id}`" :class="['constellation-edge',{'is-dim':!node.enabled,'is-selected':selectedId===node.id}]"><path :d="path(node)" class="edge-base"/><path v-if="node.enabled" :d="path(node)" class="edge-flow" :style="{animationDelay:`-${node.index*.7}s`,animationDuration:`${7+node.index%5}s`}"/></g>
            <g class="map-core"><circle cx="450" cy="280" r="66" class="core-ring outer"/><circle cx="450" cy="280" r="55" class="core-disc"/><path d="M450 246l4.4 10.6L465 261l-10.6 4.4L450 276l-4.4-10.6L435 261l10.6-4.4Z" fill="var(--accent)"/><text x="450" y="299" class="core-english">AstraLink</text><circle cx="450" cy="214" r="3" class="core-satellite"/></g>
            <g v-for="node in graph.nodes" :key="node.id" :class="['constellation-node',{'is-selected':selectedId===node.id,'is-disabled':!node.enabled}]" role="button" tabindex="0" :aria-label="`${node.name}，${nodeStatus(node.id)}，点击查看节点`" :aria-pressed="selectedId===node.id" @click="pick(node.id)" @keydown.enter="pick(node.id)" @keydown.space.prevent="pick(node.id)"><title>{{ node.name }} · {{ node.group }} · {{ nodeStatus(node.id) }}</title><circle :cx="node.x" :cy="node.y" r="23" class="node-hit"/><circle :cx="node.x" :cy="node.y" :r="selectedId===node.id?14:10" class="node-aura"/><circle :cx="node.x" :cy="node.y" r="4" class="node-dot"/><text :x="node.x" :y="node.y+31" class="node-name">{{ node.name.length>14?node.name.slice(0,13)+'…':node.name }}</text><text v-if="graph.nodes.length<11" :x="node.x" :y="node.y+45" class="node-group">{{ node.group }}</text></g>
          </svg>
          <div v-if="!graph.nodes.length" class="map-empty"><p>你的连接宇宙，等待第一颗星。</p><span>No nodes yet. Start your constellation.</span><button class="text-button" @click="emit('navigate','import')">导入已有连接 <Icon name="arrow" :size="14"/></button></div>
          <div class="map-coordinate">{{ webMode ? 'SERVER WORKSPACE' : 'LOCAL WORKSPACE' }} <span>01 / NEXUS</span></div>
        </div>
        <div class="constellation-bottom"><span><i class="legend-dot"/>已启用 <i class="legend-dot disabled"/>已停用 <i class="legend-line"/>配置关系</span><small>{{ graph.hiddenCount ? `展示 ${graph.nodes.length} / ${summary.total} 个节点` : '星图表示配置关系，不代表实时在线' }}</small><button class="text-button" @click="emit('navigate','servers')">节点管理 <Icon name="arrow" :size="13"/></button></div>
      </section>
      <aside class="dashboard-rail">
        <section class="nexus-service"><div class="rail-heading"><span class="section-kicker">THE BRIDGE</span><Icon name="terminal" :size="18"/></div><h2>连接中枢 <small>MCP Gateway</small></h2><div :class="['service-state',{running:ready&&state.engine.running}]"><span class="service-beacon"><i/><i/></span><div><strong>{{ !state.desktop?'桌面应用未连接':state.engine.running?'服务运行中':'等待启动' }}</strong><span>{{ state.engine.running?'Ready for your AI clients':'Your universe, one gateway.' }}</span></div></div><p>{{ webMode ? 'MCP 仅在服务器内部访问。' : '让 AI 客户端与服务器，在同一个入口相遇。' }}</p><div v-if="!webMode" class="service-endpoint"><span>ENDPOINT</span><code>{{ state.engine.running?state.engine.endpoint:'启动服务后可用' }}</code></div><button class="button primary full-width" :disabled="!ready||state.engineBusy||!!state.mutation" @click="dock.setEngine(!state.engine.running)"><Icon :name="state.engine.running?'stop':'play'" :size="14"/>{{ state.engineBusy?'处理中…':state.engine.running?'停止 MCP 服务':'启动 MCP 服务' }}</button><button class="service-setup" @click="emit('navigate','settings')">{{ webMode ? '管理配置备份' : '配置 AI 客户端' }} <Icon name="arrow" :size="13"/></button></section>
        <section class="selected-node-card"><div class="rail-heading"><span class="section-kicker">NODE INSPECTOR</span><Icon name="server" :size="16"/></div><template v-if="selected"><h3>{{ selected.name }}</h3><p class="mono">{{ selected.host }}:{{ selected.port }}</p><div class="node-inspector-meta"><span>{{ selected.group||'未分组' }}</span><span>{{ nodeStatus(selected.id) }}</span></div><button class="text-button" @click="emit('openServer',selected.id)">打开节点详情 <Icon name="arrow" :size="14"/></button></template><template v-else><h3>每个节点，都有自己的轨道。</h3><p>选择星图节点，查看连接信息。</p></template></section>
      </aside>
    </div>
    <div class="dashboard-bottom-grid"><section class="activity-card"><div class="dashboard-section-heading"><div><span class="section-kicker">SIGNALS</span><h2>最近动态 <small>Recent activity</small></h2></div><button class="text-button" @click="emit('navigate','logs')">全部日志 <Icon name="arrow" :size="13"/></button></div><div v-if="recent.length" class="activity-list"><div v-for="entry in recent" :key="entry.id" class="activity-item"><span :class="['activity-dot',entry.level]"/><p>{{ entry.message }}<small>{{ entry.profileId?state.profiles.find(p=>p.id===entry.profileId)?.name||'连接节点':'本机工作空间' }}</small></p><time>{{ new Date(entry.timestamp).toLocaleTimeString() }}</time></div></div><div v-else class="activity-empty"><Icon name="logs" :size="22"/><p>{{ state.runtimeLogs.error||'连接与认证过程，会在这里留下轨迹。' }}<span>{{ state.desktop?'启动 MCP 或测试连接后即可查看。':'启动桌面应用，载入你的真实连接。' }}</span></p></div></section><section class="group-card"><div class="dashboard-section-heading"><div><span class="section-kicker">CLUSTERS</span><h2>连接分组 <small>Your clusters</small></h2></div><span class="group-total">{{ summary.groups }}</span></div><div v-if="groups.length" class="dashboard-group-list"><div v-for="([name,count],index) in groups" :key="name"><span class="cluster-number">0{{ index+1 }}</span><strong>{{ name }}</strong><span class="cluster-bar"><i :style="{width:`${count/summary.total*100}%`}"/></span><b>{{ count }}</b></div></div><p v-else class="activity-empty">为节点添加分组，组织你的连接空间。</p><button class="text-button" @click="emit('navigate','servers')">探索全部连接 <Icon name="arrow" :size="13"/></button></section></div>
    <footer class="dashboard-signature"><NexusMark :size="16"/><span>AstraLink</span><span>一处连接，万物可达。<small>One nexus. Infinite connections.</small></span></footer>
  </div>
</template>
