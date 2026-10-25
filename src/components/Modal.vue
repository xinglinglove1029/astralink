<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref, useId } from 'vue'
import Icon from './Icon.vue'
const props = defineProps<{ title: string; subtitle?: string; wide?: boolean; busy?: boolean; inactive?: boolean }>()
const emit = defineEmits<{ close: [] }>()
const titleId = useId()
const panel = ref<HTMLElement>()
let previous: HTMLElement | null = null
function isTop() { return document.querySelectorAll('[data-modal]').item(document.querySelectorAll('[data-modal]').length - 1) === panel.value }
function keydown(event: KeyboardEvent) {
  if (!isTop()) return
  if (event.key === 'Escape' && !props.busy) { event.stopPropagation(); emit('close') }
  if (event.key !== 'Tab') return
  const elements = [...(panel.value?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]') ?? [])].filter((element) => element.getClientRects().length)
  const first = elements[0], last = elements.at(-1)
  if (!first) { event.preventDefault(); panel.value?.focus(); return }
  if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.value)) { event.preventDefault(); last?.focus() }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus() }
}
onMounted(async () => {
  previous = document.activeElement as HTMLElement | null
  document.addEventListener('keydown', keydown)
  await nextTick()
  panel.value?.querySelector<HTMLElement>('[autofocus], input:not([disabled]), select:not([disabled])')?.focus()
  if (!panel.value?.contains(document.activeElement)) panel.value?.focus()
})
onUnmounted(() => { document.removeEventListener('keydown', keydown); if (previous?.isConnected) previous.focus() })
</script>
<template>
  <Teleport to="body">
    <div class="modal-backdrop" @mousedown.self="!busy && emit('close')">
      <section ref="panel" data-modal role="dialog" :aria-modal="inactive ? undefined : 'true'" :aria-hidden="inactive ? 'true' : undefined" :inert="inactive" :aria-labelledby="titleId" tabindex="-1" class="modal" :class="{ 'modal-wide': wide }">
        <header class="modal-header"><div><h2 :id="titleId">{{ title }}</h2><p v-if="subtitle">{{ subtitle }}</p></div><button class="icon-button" aria-label="关闭弹窗" :disabled="busy" @click="emit('close')"><Icon name="close" /></button></header>
        <div class="modal-body"><slot /></div>
        <footer v-if="$slots.footer" class="modal-footer"><slot name="footer" /></footer>
      </section>
    </div>
  </Teleport>
</template>
