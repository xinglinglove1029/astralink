import { describe, expect, it } from 'vitest'
import { paginate, pageSelection } from '../pagination'
describe('server pagination', () => {
  const rows = Array.from({length:55}, (_,index)=>({id:String(index+1)}))
  it('paginates default 20 rows and configurable 50/100 rows', () => {
    expect(paginate(rows,1,20).items).toHaveLength(20)
    expect(paginate(rows,2,20).items[0]?.id).toBe('21')
    expect(paginate(rows,3,20).items).toHaveLength(15)
    expect(paginate(rows,2,50).items).toHaveLength(5)
    expect(paginate(rows,1,100).items).toHaveLength(55)
  })
  it('clamps deleted last pages and handles zero matches', () => {
    expect(paginate(rows.slice(0,40),3,20).page).toBe(2)
    expect(paginate([],9,20)).toMatchObject({page:1,pageCount:1,total:0,from:0,to:0,items:[]})
    expect(paginate(rows,NaN,20).page).toBe(1)
  })
  it('does not carry hidden-page selections into current-page deletion', () => {
    expect(pageSelection(['1','2','21','21'], ['21','22'])).toEqual(['21'])
  })
})
