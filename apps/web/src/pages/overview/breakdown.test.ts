import { describe, expect, it } from 'vitest'

import type { Cut, Row } from '~/generated'

import { breakdownPanel, breakdownRows, MOVERS_POOL, sharePips, shownRows } from './breakdown'

const row = (path: string, value: number, change?: number): Row => ({
  key: [{ t: 'string', v: path }],
  values: [{ t: 'count', v: value }],
  compare: [{ previous: { t: 'count', v: value - (change ?? 0) }, change }],
})

const cut = (rows: Row[]): Cut => ({
  cut: breakdownPanel('path', 10),
  totals: [{ t: 'count', v: 1000 }],
  rows,
  row_count: rows.length,
  identity_coverage: { with_identity: 0, eligible_events: 0 },
})

describe('breakdown panels', () => {
  it('requests at least the movers pool and shows fewer', () => {
    expect(breakdownPanel('path', 10).page.size).toBe(MOVERS_POOL)
    expect(breakdownPanel('country', 100).page.size).toBe(100)
  })

  it('sorts movers by absolute change and keeps quiet rows faint in the list', () => {
    const rows = breakdownRows(cut([row('/de/events', 459, 6), row('/de', 346, -3), row('/en', 57, -11), row('/new', 134, 101), row('/old', 20)]))
    const movers = shownRows(rows, 'movers', 10)
    expect(movers.map((r) => r.cell)).toEqual(['/new', '/en', '/de/events', '/de', '/old'].map((v) => ({ t: 'string', v })))
    expect(movers.map((r) => r.quiet)).toEqual([false, false, false, true, true])
    expect(movers[3]?.value).toBe(346)
  })

  it('treats a change of exactly the threshold as moving', () => {
    const [moving, quiet] = breakdownRows(cut([row('/a', 50, -5), row('/b', 50, 4)]))
    expect([moving?.quiet, quiet?.quiet]).toEqual([false, true])
  })

  it('keeps the server order and the limit in top mode', () => {
    const rows = breakdownRows(cut([row('/a', 30, 1), row('/b', 20, 20), row('/c', 10, -9)]))
    expect(shownRows(rows, 'top', 2).map((r) => r.value)).toEqual([30, 20])
    expect(rows[0]?.share).toBe(0.03)
  })

  it('rounds the share to fifths for the pips', () => {
    expect([0, 0.09, 0.1, 0.46, 0.5, 0.95, 1].map(sharePips)).toEqual([0, 0, 1, 2, 3, 5, 5])
  })
})
