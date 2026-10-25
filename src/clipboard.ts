/** Copy from an explicit user action, including plain HTTP LAN deployments. */
export async function copyText(value: string): Promise<void> {
  const fallback = () => {
    const active = document.activeElement as HTMLElement | null
    const selection = window.getSelection()
    const ranges = selection ? Array.from({ length: selection.rangeCount }, (_, i) => selection.getRangeAt(i).cloneRange()) : []
    const field = document.createElement('textarea')
    field.value = value
    field.readOnly = true
    field.setAttribute('aria-label', '复制内容')
    // Keep the temporary selection inside the active modal and out of view.
    field.style.cssText = 'position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none'
    const modal = [...document.querySelectorAll<HTMLElement>('[data-modal]')].filter(item => !item.inert).at(-1)
    ;(modal ?? document.body).append(field)
    try {
      field.focus({ preventScroll: true })
      field.select()
      field.setSelectionRange(0, value.length)
      if (!document.execCommand('copy')) throw new Error('剪贴板不可用，请手动复制')
    } finally {
      field.value = ''
      field.remove()
      active?.focus({ preventScroll: true })
      if (selection) { selection.removeAllRanges(); for (const range of ranges) selection.addRange(range) }
    }
  }
  if (typeof navigator.clipboard?.writeText !== 'function') { fallback(); return }
  try { await navigator.clipboard.writeText(value) }
  catch { fallback() }
}
