import { describe, expect, it } from 'vitest'

import { durationParts, millisOf } from './funnel-duration'

describe('durationParts', () => {
  it('keeps the two largest units', () => {
    expect(durationParts(159_000)).toEqual([
      { unit: 'min', value: 2 },
      { unit: 's', value: 39 },
    ])
    expect(durationParts(3 * 86_400_000 + 4 * 3_600_000 + 59 * 60_000)).toEqual([
      { unit: 'd', value: 3 },
      { unit: 'h', value: 4 },
    ])
  })

  it('has no parts below a second, and drops a zero second unit', () => {
    expect(durationParts(999)).toEqual([])
    expect(durationParts(120_000)).toEqual([{ unit: 'min', value: 2 }])
  })

  it('reads no duration from an unavailable metric', () => {
    expect(millisOf({ t: 'unavailable', reason: 'no_values' })).toBeNull()
    expect(millisOf({ t: 'millis', v: 5 })).toBe(5)
  })
})
