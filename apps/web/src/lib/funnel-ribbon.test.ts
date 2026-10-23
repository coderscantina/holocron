import { describe, expect, it } from 'vitest'

import type { FunnelLeak } from '~/generated'
import { flowsOf, previousDays, ribbon, worstLeak } from '~/lib/funnel-ribbon'

const leak = (exited: number, back: number, pages: [string, number][], other: number): FunnelLeak => ({
  exited,
  back,
  next: pages.map(([path, identities]) => ({ path, identities })),
  other,
  sessions: [],
})

describe('flowsOf', () => {
  it('lists the destinations that hold leavers, left and back first', () => {
    expect(flowsOf(leak(5, 0, [['/about', 2]], 1), 8).map((flow) => flow.kind)).toEqual(['exited', 'page', 'other'])
  })

  it('shows a drop without leak data as one flow', () => {
    expect(flowsOf(undefined, 4)).toEqual([{ kind: 'other', path: null, identities: 4 }])
    expect(flowsOf(undefined, 0)).toEqual([])
  })
})

describe('ribbon', () => {
  const drawn = ribbon([100, 40, 30], [leak(50, 10, [], 0), leak(10, 0, [], 0)], { width: 600, height: 400 })

  it('narrows each column to its share of step 1', () => {
    const [first, second, third] = drawn.steps
    expect(second!.height / first!.height).toBeCloseTo(0.4)
    expect(third!.height / first!.height).toBeCloseTo(0.3)
    expect(third!.x + third!.width).toBe(600)
  })

  it('peels the leavers off below the continuing band, stacked by destination', () => {
    const [left, back] = drawn.leaks[0]!.flows
    expect(drawn.leaks[0]).toMatchObject({ step: 0, dropped: 60, rate: 0.6 })
    expect(left!.labelY).toBeLessThan(back!.labelY)
    expect(left!.labelY).toBeGreaterThan(drawn.steps[0]!.y + drawn.steps[0]!.height)
  })

  it('fits the ribbon and the deepest leak into the height', () => {
    const bottom = Math.max(...drawn.leaks.flatMap((l) => l.flows.map((flow) => flow.labelY)))
    expect(bottom).toBeLessThan(400)
  })
})

describe('previousDays', () => {
  it('takes as many local days right before the range', () => {
    const start = Date.parse('2026-09-30T22:00:00Z')
    const end = Date.parse('2026-10-07T22:00:00Z')
    expect(previousDays({ start, end, timezone: 'Europe/Vienna' })).toEqual({ kind: 'days', start: '2026-09-24', end: '2026-09-30' })
  })
})

describe('worstLeak', () => {
  it('picks the step losing the largest share, not the most people', () => {
    expect(worstLeak(ribbon([100, 40, 10], [], { width: 300, height: 200 }).leaks)?.step).toBe(1)
    expect(worstLeak(ribbon([10, 10], [], { width: 300, height: 200 }).leaks)).toBeNull()
  })
})
