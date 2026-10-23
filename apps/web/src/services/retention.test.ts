import { describe, expect, it } from 'vitest'

import { decodeState, encodeState } from './exploration'
import { intervalsOf, retentionState, setCohortPeriod, setIntervals } from './retention'

const site = '0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01'

describe('retention states', () => {
  it('round-trip through the URL with their report and version', () => {
    const state = retentionState(site, 'Europe/Vienna')
    const decoded = decodeState(encodeState(state))
    expect(decoded).toEqual({ ok: true, state })
    expect(state.report).toEqual({ kind: 'retention', v: 1, intervals: 8 })
  })

  it('switch cohort periods with a matching range and clamp offsets to the period limit', () => {
    const daily = setCohortPeriod(retentionState(site, 'UTC'), 'day')
    expect(daily.granularity).toBe('day')
    expect(daily.range).toEqual({ kind: 'relative', unit: 'day', count: 14 })
    expect(intervalsOf(setIntervals(daily, 40))).toBe(32)
    expect(intervalsOf(setIntervals(setCohortPeriod(daily, 'week'), 60))).toBe(53)
  })
})
