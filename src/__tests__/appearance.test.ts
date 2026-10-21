import { describe, expect, it } from 'vitest'
import { readAppearance, saveAppearance } from '../appearance'
describe('appearance preferences', () => {
  it('defaults to Nebula with motion and rejects unknown persisted themes', () => {
    expect(readAppearance(null)).toEqual({theme:'nebula',motion:true})
    expect(readAppearance('{"theme":"bad","motion":false}')).toEqual({theme:'nebula',motion:false})
    expect(readAppearance('broken')).toEqual({theme:'nebula',motion:true})
    expect(readAppearance('{"theme":"cloud","motion":true}').theme).toBe('cloud')
  })
  it('persists appearance without throwing on unavailable storage', () => {
    let saved=''
    expect(saveAppearance({theme:'midnight',motion:false},{setItem:(_key,value)=>{saved=value}})).toBe(true)
    expect(readAppearance(saved)).toEqual({theme:'midnight',motion:false})
    expect(saveAppearance({theme:'cloud',motion:true},{setItem:()=>{throw Error('denied')}})).toBe(false)
  })
})
