import { describe, expect, it } from 'vitest'

import type { FunnelGroup, FunnelResult, FunnelStep, Metric } from '~/generated'

import { GROUPS_SHOWN, splitRows } from './funnel-split'

const step = (reached: number): FunnelStep => ({ reached }) as FunnelStep
const ratio = (v: number): Metric => ({ t: 'ratio', v, numerator: 0, denominator: 0 })
const group = (name: string, first: number, last: number): FunnelGroup => ({
  value: { t: 'string', v: name },
  steps: [step(first), step(last)],
  conversion: ratio(last / first),
  durations: [],
})
const result = (count: number): FunnelResult =>
  ({
    steps: [step(1000), step(100)],
    conversion: ratio(0.1),
    durations: [],
    groups: Array.from({ length: count }, (_, i) => group(`g${i}`, 500 - i, 50)),
    groups_truncated: false,
  }) as unknown as FunnelResult

describe('splitRows', () => {
  it('lists all traffic first, then the top ten groups in the server order', () => {
    const { rows, hidden } = splitRows(result(12), false)
    expect(rows).toHaveLength(GROUPS_SHOWN + 1)
    expect(rows[0]).toMatchObject({ cell: null, entered: 1000 })
    expect(rows.slice(1).map((row) => row.entered)).toEqual(Array.from({ length: 10 }, (_, i) => 500 - i))
    expect(hidden).toBe(2)
  })

  it('lists every group on request, and nothing is hidden with ten or fewer', () => {
    expect(splitRows(result(12), true)).toMatchObject({ hidden: 0, rows: { length: 13 } })
    expect(splitRows(result(10), false).hidden).toBe(0)
  })
})
