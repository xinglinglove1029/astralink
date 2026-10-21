<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import Modal from './Modal.vue'
import Icon from './Icon.vue'
import SecretInput from './SecretInput.vue'
import type { AuthChallenge } from '../types'
const props = defineProps<{ challenge: AuthChallenge; busy: boolean; queueLength: number; error?: string }>()
const emit = defineEmits<{ submit: [code: string]; cancel: [] }>()
const code = ref('')
const now = ref(Date.now())
const ticker = window.setInterval(() => { now.value = Date.now() }, 1000)
const seconds = computed(() => Math.max(0, Math.ceil((props.challenge.expiresAt - now.value) / 1000)))
const supported = computed(() => props.challenge.prompts.length === 1)
onUnmounted(() => { window.clearInterval(ticker); code.value = '' })
function submit() { if (seconds.value > 0 && supported.value && code.value.trim() && !props.busy) emit('submit', code.value) }
</script>
<template>
  <Modal title="完成服务器认证" :subtitle="`${challenge.connectionName} · ${challenge.username}@${challenge.host}`" :busy="busy" @close="emit('cancel')">
    <form id="otp-form" class="otp-form" @submit.prevent="submit">
      <div class="otp-symbol"><Icon name="shield" :size="32" /></div>
      <p class="otp-description">服务器正在等待验证信息。提交后将继续当前连接请求。</p>
      <div v-if="challenge.instructions" class="server-instructions">{{ challenge.instructions }}</div>
      <label v-if="supported" class="field"><span>{{ challenge.prompts[0]!.prompt || '验证码' }}</span><SecretInput v-if="!challenge.prompts[0]!.echo" v-model="code" reveal-label="验证码" autofocus autocomplete="one-time-code" :disabled="busy || seconds === 0" placeholder="输入服务器要求的验证信息" /><input v-else v-model="code" type="text" autofocus autocomplete="one-time-code" :disabled="busy || seconds === 0" placeholder="输入服务器要求的验证信息" /><small>仅用于本次认证，不保存。</small></label>
      <div v-else class="inline-note warning"><Icon name="alert" :size="17" />当前服务器返回 {{ challenge.prompts.length }} 个认证提示；此版本支持单个提示，请取消后使用其他认证方式。</div>
      <div class="otp-time" :class="{ 'danger-text': seconds === 0 }"><Icon name="clock" :size="15" />{{ seconds > 0 ? `${seconds} 秒后过期` : '认证请求已过期，请关闭并重新测试连接' }}<span v-if="queueLength > 1">另有 {{ queueLength - 1 }} 个请求等待</span></div>
      <p v-if="error" class="field-error" role="alert">{{ error }}</p>
    </form>
    <template #footer><button class="button secondary" :disabled="busy" @click="emit('cancel')">{{ seconds === 0 ? '关闭' : '取消认证' }}</button><button class="button primary" type="submit" form="otp-form" :disabled="busy || seconds === 0 || !supported || !code.trim()"><span v-if="busy" class="spinner" />{{ busy ? '提交中…' : '提交验证信息' }}</button></template>
  </Modal>
</template>
