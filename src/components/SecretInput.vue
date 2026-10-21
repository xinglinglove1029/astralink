<script setup lang="ts">
import { onUnmounted, watch } from 'vue'
import { useSecretVisibility } from '../secretVisibility'
import Icon from './Icon.vue'
defineOptions({ inheritAttrs: false })
const model = defineModel<string | undefined>()
const props = withDefaults(defineProps<{ disabled?: boolean; revealLabel?: string; loadSaved?: () => Promise<string | null> }>(), { disabled: false, revealLabel: '密码' })
const { visible, revealed, loading, error, inputType, toggleLabel, toggle, clear } = useSecretVisibility(props.revealLabel)
onUnmounted(clear)
watch(model, () => { if (revealed.value) clear() })
function handleInput(event: Event) { model.value = (event.target as HTMLInputElement).value }
</script>
<template>
  <div class="secret-input" :class="{ 'has-error': !!error }">
    <input v-bind="$attrs" :value="revealed || model || ''" :type="inputType" :readonly="!!revealed" :disabled="disabled || loading" @input="handleInput" />
    <button type="button" class="secret-visibility" :disabled="disabled || loading" :aria-label="toggleLabel" :title="toggleLabel" :aria-pressed="visible" @click="toggle(!model ? loadSaved : undefined)"><Icon :name="visible ? 'eyeOff' : 'eye'" :size="16" /></button>
    <small v-if="error" class="secret-error field-error" role="alert">{{ error }}</small>
  </div>
</template>
