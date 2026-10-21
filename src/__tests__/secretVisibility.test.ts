import { describe, expect, it } from 'vitest'
import { useSecretVisibility } from '../secretVisibility'

describe('secret visibility', () => {
  it('starts hidden and exposes a toggle with an accessible label', () => {
    const field = useSecretVisibility('密码')
    expect(field.inputType.value).toBe('password')
    expect(field.toggleLabel.value).toBe('显示密码')
    field.toggle()
    expect(field.inputType.value).toBe('text')
    expect(field.toggleLabel.value).toBe('隐藏密码')
    field.toggle()
    expect(field.inputType.value).toBe('password')
  })
  it('loads a stored value only on reveal and scrubs it when hidden', async () => {
    const field = useSecretVisibility('密码')
    let reads = 0
    await field.toggle(async () => { reads++; return 'saved-secret' })
    expect(reads).toBe(1)
    expect(field.revealed.value).toBe('saved-secret')
    expect(field.inputType.value).toBe('text')
    await field.toggle()
    expect(field.revealed.value).toBe('')
    expect(field.inputType.value).toBe('password')
  })

  it('ignores a late vault response after the editor is disposed', async () => {
    let finish!: (value: string) => void
    const field = useSecretVisibility('密码')
    const pending = field.toggle(() => new Promise(resolve => { finish = resolve }))
    field.clear()
    finish('saved-secret')
    await pending
    expect(field.revealed.value).toBe('')
    expect(field.visible.value).toBe(false)
  })
})
