import { describe, expect, it } from 'vitest'
import { filterRuntimeLogs, formatRuntimeLogs } from '../runtimeLogs'
import type { RuntimeLogEntry } from '../types'
const entries: RuntimeLogEntry[] = [
  { id:1,timestamp:1700000000000,level:'info',source:'auth',code:'otp_auto',message:'验证码已自动提交',profileId:'profile',requestId:'request' },
  { id:2,timestamp:1700000001000,level:'warn',source:'test',code:'test_target_menu',message:'JumpServer 资产菜单' },
];
describe('runtime log viewer', () => {
  it('filters level, profile and keyword and shows newest first without mutating logs', () => {
    expect(filterRuntimeLogs(entries, '', '', '').map(e=>e.id)).toEqual([2,1])
    expect(filterRuntimeLogs(entries, 'warn', '', '')).toEqual([entries[1]])
    expect(filterRuntimeLogs(entries, '', 'profile', '验证码')).toEqual([entries[0]])
    expect(filterRuntimeLogs(entries, '', '', 'jumPserver')).toEqual([entries[1]])
    expect(entries[0]?.id).toBe(1)
  })
  it('formats copied logs with time, phase and correlation identifiers', () => {
    const text = formatRuntimeLogs(entries)
    expect(text).toContain('[INFO] auth otp_auto')
    expect(text).toContain('profile=profile request=request')
    expect(text).toContain('JumpServer 资产菜单')
  })
})
