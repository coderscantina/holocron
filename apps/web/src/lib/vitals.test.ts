import { describe, expect, it } from 'vitest'

import { track } from '~/lib/vitals'

describe('track', () => {
  it('places bands and values on a scale that runs half again past the needs-improvement bound', () => {
    // LCP: good to 2500 ms, needs improvement to 4000 ms, so the track ends at 6000 ms.
    const scale = track('lcp', [
      { t: 'millis', v: 1500 },
      { t: 'millis', v: 3000 },
      { t: 'millis', v: 6000 },
    ])
    expect(scale.good).toBeCloseTo(41.67, 1)
    expect(scale.improve).toBeCloseTo(66.67, 1)
    expect(scale.points).toEqual([25, 50, 100])
  })

  it('reads CLS scores, clamps values past the end and keeps unavailable values out', () => {
    const scale = track('cls', [
      { t: 'decimal', v: '0.0375' },
      { t: 'decimal', v: '2' },
      { t: 'unavailable', reason: 'no_values' },
    ])
    expect(scale.points).toEqual([10, 100, null])
  })
})
