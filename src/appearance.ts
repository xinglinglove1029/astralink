import { reactive } from 'vue'
export const themes = [
  { id:'cloud',name:'云白',english:'Cloud',description:'通透明亮，专注日常',color:'#e6eef8' },
  { id:'midnight',name:'曜夜',english:'Midnight',description:'深邃石墨，沉浸工作',color:'#233042' },
  { id:'nebula',name:'星海',english:'Nebula',description:'蓝绿星光，连接万物',color:'#126b78' },
] as const
export type Theme = typeof themes[number]['id']
export interface Appearance { theme: Theme; motion: boolean }
const key='astralink.appearance.v1'
export function readAppearance(text: string|null): Appearance {
  try { const data=JSON.parse(text??'{}'); return {theme:themes.some(t=>t.id===data?.theme)?data.theme:'nebula',motion:typeof data?.motion==='boolean'?data.motion:true} }
  catch { return {theme:'nebula',motion:true} }
}
export function saveAppearance(value: Appearance, storage: Pick<Storage,'setItem'>): boolean {
  try { storage.setItem(key,JSON.stringify(value)); return true } catch { return false }
}
function restored() { try { return readAppearance(typeof localStorage==='undefined'?null:localStorage.getItem(key)) } catch { return readAppearance(null) } }
export const appearance=reactive(restored())
export const appearanceSaveState=reactive({persisted:true})
function apply() {
  if(typeof document==='undefined') return
  document.documentElement.dataset.theme=appearance.theme
  document.documentElement.dataset.motion=appearance.motion?'on':'off'
  document.documentElement.style.colorScheme=appearance.theme==='cloud'?'light':'dark'
}
export function initializeAppearance() { apply() }
function persist() { apply(); try { appearanceSaveState.persisted=saveAppearance(appearance,localStorage) } catch { appearanceSaveState.persisted=false } }
export function setTheme(theme: Theme) { if(!themes.some(t=>t.id===theme))return; appearance.theme=theme; persist() }
export function setMotion(enabled: boolean) { appearance.motion=enabled; persist() }
