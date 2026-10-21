import type { ConnectionProfile, ConnectionTest } from './types'
export function dashboardSummary(profiles:ConnectionProfile[], results:Record<string,ConnectionTest>) {
  const tests=profiles.map(p=>results[p.id]).filter(Boolean)
  return {total:profiles.length,enabled:profiles.filter(p=>p.enabled).length,groups:new Set(profiles.map(p=>p.group||'未分组')).size,tested:tests.length,passed:tests.filter(t=>t?.status==='connected').length}
}
export interface ConstellationNode { id:string; name:string; group:string; enabled:boolean; x:number; y:number; parentId:string; index:number }
export function buildConstellation(profiles:ConnectionProfile[]) {
  const selected:ConnectionProfile[]=[]
  const seen=new Set<string>()
  const byId=new Map(profiles.map(p=>[p.id,p]))
  function add(p:ConnectionProfile,visiting=new Set<string>()) {
    if(seen.has(p.id)||selected.length>=20||visiting.has(p.id))return
    visiting.add(p.id)
    const parent=p.jumpHostId?byId.get(p.jumpHostId):undefined
    if(parent)add(parent,visiting)
    if(selected.length<20&&!seen.has(p.id)){selected.push(p);seen.add(p.id)}
  }
  for(const profile of profiles)add(profile)
  const visible=selected, ids=new Set(visible.map(p=>p.id))
  const nodes:ConstellationNode[]=visible.map((p,index)=>{
    const ring=index<10?0:1, count=ring===0?Math.min(visible.length,10):visible.length-10
    const theta=(index%10)/count*Math.PI*2-Math.PI/2+(ring?0.22:0)
    return {id:p.id,name:p.name,group:p.group||'未分组',enabled:p.enabled,index,
      x:450+Math.cos(theta)*(ring?355:240), y:280+Math.sin(theta)*(ring?212:150),parentId:p.jumpHostId&&ids.has(p.jumpHostId)?p.jumpHostId:'core'}
  })
  return {nodes,hiddenCount:Math.max(0,profiles.length-nodes.length)}
}
