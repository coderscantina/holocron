import type { Metric, RetentionResult } from '~/generated'

/**
 * Layout and colour math of the retention heatmap. Offset 0 is the cohort period itself and
 * always 100% (0003), so the grid starts at offset 1; cells that have not started yet are left
 * out, which gives the grid its triangle.
 */

/** Shades above "none", evenly spaced in lightness by mixing the accent into the card. */
export const HEAT_STEPS = 5

export interface HeatCell {
  offset: number
  start: number
  end: number
  active: number
  /** `null` for a size-0 cohort. */
  rate: number | null
  complete: boolean
  /** 0 for none or no rate, 1 to `HEAT_STEPS` by rate against the grid's highest. */
  step: number
}

export interface HeatRow {
  start: number
  local: string
  size: number
  cells: HeatCell[]
}

export interface Heatmap {
  rows: HeatRow[]
  /** Offsets 1 to `columns` have a column. */
  columns: number
  /** The highest rate in the grid, the top of the colour scale and of the curve chart. */
  max: number
}

export const ratio = (metric: Metric | undefined): number | null => (metric?.t === 'ratio' ? metric.v : null)

/** A rate's shade: 0 for none, otherwise 1 to `HEAT_STEPS` in equal parts of `max`. */
export function heatStep(rate: number | null, max: number): number {
  if (rate === null || rate <= 0 || max <= 0) return 0
  return Math.min(HEAT_STEPS, Math.max(1, Math.ceil((rate / max) * HEAT_STEPS)))
}

/**
 * A shade as a CSS colour: the accent mixed into the card, or the passive fill for none. Mixed in
 * OKLab, so lightness steps evenly and the hue never swings through a third colour. The top
 * share comes from `--heat-max`, set per mode so primary text stays readable on every shade.
 */
export const heatFill = (step: number): string =>
  step <= 0 ? 'var(--fill)' : `color-mix(in oklab, var(--accent) calc(var(--heat-max) * ${step / HEAT_STEPS}), var(--card))`

/** The grid of a retention result: one row per cohort, cells from offset 1 up to those started by the watermark. */
export function heatmap(result: RetentionResult): Heatmap {
  const started = result.cohorts.map((cohort) =>
    cohort.cells.flatMap((cell, offset) => (offset > 0 && cell.start < result.watermark ? [{ cell, offset }] : []))
  )
  const max = Math.max(0, ...started.flat().map(({ cell }) => ratio(cell.rate) ?? 0))
  const rows = result.cohorts.map((cohort, index) => ({
    start: cohort.start,
    local: cohort.local,
    size: cohort.size,
    cells: (started[index] ?? []).map(({ cell, offset }) => {
      const rate = ratio(cell.rate)
      return { offset, start: cell.start, end: cell.end, active: cell.active, rate, complete: cell.complete, step: heatStep(rate, max) }
    }),
  }))
  return { rows, columns: Math.max(0, result.averages.length - 1), max }
}

export interface Trend {
  /** Local start date of the first recent cohort. */
  since: string
  /** Recent over earlier, minus 1. */
  change: number
}

/** Fewer people than this on either side make a comparison noise. */
export const TREND_MIN_PEOPLE = 20

/**
 * How the newer half of the cohorts with a complete interval at `offset` compares with the older
 * half, each pooled as active over size. `null` with fewer than two such cohorts, fewer than
 * `TREND_MIN_PEOPLE` on a side, or none kept in the older half.
 */
export function trend(result: RetentionResult, offset: number): Trend | null {
  const measured = result.cohorts.filter((cohort) => cohort.size > 0 && cohort.cells[offset]?.complete)
  const split = Math.ceil(measured.length / 2)
  const pool = (cohorts: typeof measured) => ({
    active: cohorts.reduce((sum, cohort) => sum + (cohort.cells[offset]?.active ?? 0), 0),
    size: cohorts.reduce((sum, cohort) => sum + cohort.size, 0),
  })
  const earlier = pool(measured.slice(0, split))
  const recent = measured.slice(split)
  const later = pool(recent)
  const first = recent[0]
  if (!first || earlier.active === 0 || Math.min(earlier.size, later.size) < TREND_MIN_PEOPLE) return null
  return { since: first.local, change: later.active / later.size / (earlier.active / earlier.size) - 1 }
}

export interface ChartPoint {
  x: number
  y: number
}

/** Points of rates on offsets 1 to `columns` in a `width` by `height` box, 0 at the bottom and `max` at the top. Gaps stay out. */
export function curvePoints(
  rates: readonly { offset: number; rate: number | null }[],
  columns: number,
  max: number,
  width: number,
  height: number
): ChartPoint[] {
  const top = max > 0 ? max : 1
  const x = (offset: number) => (columns <= 1 ? width / 2 : ((offset - 1) / (columns - 1)) * width)
  return rates.flatMap(({ offset, rate }) =>
    rate === null ? [] : [{ x: x(offset), y: height - (Math.min(rate, top) / top) * height }]
  )
}

export const pointList = (points: readonly ChartPoint[]): string =>
  points.map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(' ')

export interface GridPosition {
  row: number
  column: number
}

/**
 * The cell an arrow, Home or End key moves to in rows of `lengths` cells. Up and down keep the
 * column, falling back to a shorter row's last cell, and skip rows without cells. `null` when
 * the key does not move.
 */
export function moveInGrid(lengths: readonly number[], from: GridPosition, key: string): GridPosition | null {
  const length = lengths[from.row] ?? 0
  const vertical = (step: number) => {
    for (let row = from.row + step; row >= 0 && row < lengths.length; row += step) {
      const cells = lengths[row] ?? 0
      if (cells > 0) return { row, column: Math.min(from.column, cells - 1) }
    }
    return null
  }
  switch (key) {
    case 'ArrowLeft':
      return from.column > 0 ? { row: from.row, column: from.column - 1 } : null
    case 'ArrowRight':
      return from.column < length - 1 ? { row: from.row, column: from.column + 1 } : null
    case 'ArrowUp':
      return vertical(-1)
    case 'ArrowDown':
      return vertical(1)
    case 'Home':
      return from.column > 0 ? { row: from.row, column: 0 } : null
    case 'End':
      return from.column < length - 1 ? { row: from.row, column: length - 1 } : null
    default:
      return null
  }
}
