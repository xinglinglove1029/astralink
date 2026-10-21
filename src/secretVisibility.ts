import { computed, ref } from 'vue'

export function useSecretVisibility(label: string) {
  const visible = ref(false)
  const revealed = ref('')
  const loading = ref(false)
  const error = ref('')
  let generation = 0
  function clear() {
    generation++
    revealed.value = ''
    visible.value = false
    loading.value = false
    error.value = ''
  }
  async function toggle(loadSaved?: () => Promise<string | null>) {
    if (visible.value) { clear(); return }
    if (!loadSaved) { visible.value = true; return }
    if (loading.value) return
    const current = ++generation
    loading.value = true
    error.value = ''
    try {
      const value = await loadSaved()
      if (generation !== current) return
      if (!value) { error.value = '未找到已保存的凭据'; return }
      revealed.value = value
      visible.value = true
    } catch {
      if (generation === current) error.value = '读取已保存的凭据失败'
    } finally {
      if (generation === current) loading.value = false
    }
  }
  return {
    visible,
    revealed,
    loading,
    error,
    inputType: computed(() => visible.value ? 'text' : 'password'),
    toggleLabel: computed(() => `${visible.value ? '隐藏' : '显示'}${label}`),
    toggle,
    clear,
  }
}
