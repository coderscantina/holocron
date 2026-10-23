import type { Cell, Cut, Field, Metric, Panel } from '~/generated'
import { PAGE_SIZE_DEFAULT } from '~/generated/limits'

/**
 * The overview's breakdown tables (design contract, Tables and breakdowns): which panels the
 * overview requests with its one exploration, and the rows each shows in Top and Movers mode.
 * Nothing here formats or translates.
 */

export type BreakdownMode = 'top' | 'movers'

/** Below this absolute change in a count a row is quiet: faint, but kept in the movers list. */
export const QUIET_CHANGE = 5

/**
 * Rows a panel requests at least. Movers rank by change within the requested page, and the
 * server sorts pages by value, so a panel that shows ten requests more to find its movers.
 */
export const MOVERS_POOL = PAGE_SIZE_DEFAULT

/** One overview panel: visitor-days by one field, `shown` rows from a page of at least the pool. */
export function breakdownPanel(field: Field, shown: number): Panel {
  return {
    measures: ['visitor_days'],
    dimensions: [field],
    sort: [{ by: 'visitor_days', dir: 'desc' }],
    page: { size: Math.max(shown, MOVERS_POOL), index: 0 },
  }
}

export interface BreakdownRow {
  cell: Cell
  value: number
  /** Of the panel total, 0 to 1. */
  share: number
  /** Against the comparison; `null` without one or when the baseline is unavailable. */
  change: number | null
  quiet: boolean
}

const count = (metric: Metric | undefined) => (metric?.t === 'count' ? metric.v : 0)

/** Every row of a panel's cut in server order, by value. */
export function breakdownRows(cut: Cut): BreakdownRow[] {
  const total = count(cut.totals[0])
  return cut.rows.flatMap((row) => {
    const cell = row.key[0]
    if (!cell) return []
    const value = count(row.values[0])
    const change = row.compare?.[0]?.change ?? null
    return [
      {
        cell,
        value,
        share: total > 0 ? value / total : 0,
        change,
        quiet: change === null || Math.abs(change) < QUIET_CHANGE,
      },
    ]
  })
}

/**
 * The rows a mode shows: Top keeps the server's order, Movers sorts by absolute change with
 * rows without a change last. Both keep `limit` rows.
 */
export function shownRows(rows: BreakdownRow[], mode: BreakdownMode, limit: number): BreakdownRow[] {
  if (mode === 'top') return rows.slice(0, limit)
  const size = (row: BreakdownRow) => (row.change === null ? -1 : Math.abs(row.change))
  return rows.toSorted((a, b) => size(b) - size(a)).slice(0, limit)
}

/** Filled pips of the 5-pip share meter: the share rounded to fifths. */
export const sharePips = (share: number) => Math.round(Math.min(1, Math.max(0, share)) * 5)
