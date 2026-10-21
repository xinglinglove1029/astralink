<script setup lang="ts">
import Icon from './Icon.vue'
import { authLabels, connectionResultLabel } from '../domain'
import type { ConnectionProfile, ConnectionTest } from '../types'
defineProps<{ profile?: ConnectionProfile; result?: ConnectionTest; testing: boolean; authPending: boolean; canTest: boolean; busy: boolean }>()
const emit = defineEmits<{ edit: [profile: ConnectionProfile]; delete: [profile: ConnectionProfile]; test: [id: string] }>()
const stages: Record<string, string> = { config: '配置校验', network: '网络连接', authentication: '身份认证', auth: '身份认证', target: '目标资产', command: '命令执行', connected: '连接完成', request: '请求失败', connect: '建立连接' }
</script>
<template>
  <aside class="connection-panel card">
    <div class="panel-heading"><span>连接详情</span><Icon name="terminal" :size="17" /></div>
    <template v-if="profile">
      <div class="connection-identity"><span class="detail-avatar"><Icon name="server" :size="27" /></span><h3>{{ profile.name }}</h3><p class="mono">{{ profile.host }}:{{ profile.port }}</p><span class="badge neutral">{{ profile.group || '未分组' }}</span></div>
      <dl class="detail-list"><div><dt>用户名</dt><dd class="mono">{{ profile.username }}</dd></div><div><dt>认证方式</dt><dd>{{ authLabels[profile.authType] }}</dd></div><div><dt>传输方式</dt><dd>{{ profile.transportMode === 'exec' ? '命令执行' : '交互 Shell' }}</dd></div><div><dt>交互认证</dt><dd>{{ profile.tryKeyboard ? '已开启' : '未开启' }}</dd></div><div><dt>连接状态</dt><dd>{{ profile.enabled ? '已启用' : '已停用' }}</dd></div><div v-if="profile.targetHost"><dt>目标资产</dt><dd>{{ profile.targetHost }}</dd></div><div v-if="profile.targetUsername"><dt>目标账号</dt><dd>{{ profile.targetUsername }}</dd></div></dl>
      <div v-if="profile.notes" class="connection-notes"><span>备注</span><p>{{ profile.notes }}</p></div>
      <div class="connection-result" aria-live="polite">
        <div class="result-heading"><Icon :name="testing ? 'clock' : result?.status === 'connected' ? 'check' : result?.status === 'partial' ? 'shield' : result?.status === 'failed' ? 'alert' : 'terminal'" :size="16" /><strong>{{ testing ? authPending ? '等待验证信息' : '正在测试连接' : result ? '最近一次测试' : '尚未测试连接' }}</strong><span v-if="result && !testing">{{ (result.elapsedMs / 1000).toFixed(2) }}s</span></div>
        <p v-if="testing">{{ authPending ? '请在认证弹窗中输入验证码，连接测试会继续等待。' : '正在连接服务器并验证只读命令，请稍候。' }}</p>
        <template v-else-if="result"><span :class="['badge', result.status === 'connected' ? 'green' : result.status === 'failed' ? 'red' : 'amber']">{{ connectionResultLabel(result.status) }}</span><p><strong>{{ stages[result.stage] || result.stage }}</strong> · {{ result.detail }}</p><pre v-if="result.stdout">{{ result.stdout }}</pre><small>测试结果不代表持续在线状态</small></template>
        <p v-else>启动 MCP 后测试网络、认证与实际命令结果。</p>
      </div>
      <button class="button primary full-width" :disabled="!canTest || testing" @click="emit('test', profile.id)"><span v-if="testing" class="spinner" /><Icon v-else name="terminal" :size="16" />{{ testing ? '测试中…' : '测试连接' }}</button>
      <div class="detail-actions"><button class="text-button" :disabled="busy" @click="emit('edit', profile)"><Icon name="edit" :size="15" />编辑</button><button class="text-button danger-text" :disabled="busy" @click="emit('delete', profile)"><Icon name="trash" :size="15" />删除</button></div>
    </template>
    <div v-else class="detail-empty"><Icon name="server" :size="36" /><h3>连接，一目了然</h3><p>选择服务器查看配置与<br />真实连接测试结果。</p><div class="mini-terminal"><span class="terminal-dots">•••</span><p>$ hostname<br /><span>等待选择服务器…</span></p></div></div>
  </aside>
</template>
