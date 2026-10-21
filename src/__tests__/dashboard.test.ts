import { describe, expect, it } from 'vitest'
import { buildConstellation, dashboardSummary } from '../dashboard'
import { createProfile } from '../domain'
const profiles=Array.from({length:55},(_,i)=>({...createProfile(),id:`node-${i}`,name:`服务器 ${i}`,host:'example.test',username:'ops',group:i<30?'测试':'生产',enabled:i%2===0}))
describe('dashboard derives real configured relationships', () => {
  it('keeps enabled and historical test counts distinct from online state', () => {
    expect(dashboardSummary(profiles,{ 'node-1':{status:'connected',stage:'command',detail:'',elapsedMs:1}, 'node-2':{status:'partial',stage:'target',detail:'',elapsedMs:1} })).toMatchObject({total:55,enabled:28,groups:2,tested:2,passed:1})
    expect(dashboardSummary([],{})).toMatchObject({total:0,enabled:0,groups:0,tested:0,passed:0})
  })
  it('bounds graph size and creates stable positions without inventing servers', () => {
    const graph=buildConstellation(profiles)
    expect(graph.nodes.length).toBeLessThanOrEqual(20)
    expect(graph.hiddenCount).toBe(55-graph.nodes.length)
    expect(graph.nodes.every(node=>profiles.some(p=>p.id===node.id))).toBe(true)
    expect(buildConstellation(profiles)).toEqual(graph)
    expect(graph.nodes.every(node=>node.x>30&&node.x<870&&node.y>40&&node.y<520)).toBe(true)
  })
  it('routes configured jump relationships through the correct parent', () => {
    const a={...profiles[0]!,jumpHostId:undefined},b={...profiles[1]!,jumpHostId:a.id}
    const graph=buildConstellation([a,b])
    expect(graph.nodes.find(n=>n.id===b.id)?.parentId).toBe(a.id)
    expect(buildConstellation([]).nodes).toEqual([])
  })
})
