import { describe, expect, it } from 'vitest'

import type { CohortRow, Metric, RetentionCell, RetentionResult } from '~/generated'
import { HEAT_STEPS, curvePoints, heatFill, heatStep, heatmap, moveInGrid, trend } from '~/lib/retention-heatmap'

const WEEK = 7 * 86_400_000
const rate = (active: number, size: number): Metric =>
  size === 0 ? { t: 'unavailable', reason: 'no_denominator' } : { t: 'ratio', v: active / size, numerator: active, denominator: size }

/** A weekly cohort starting `week` weeks after 0 with `active` per offset, complete before `watermark`. */
const cohort = (week: number, size: number, active: number[], watermark: number): CohortRow => ({
  start: week * WEEK,
  local: `2026-01-${String(week + 1).padStart(2, '0')}`,
  size,
  coverage: { with_identity: 0, eligible_events: 0 },
  cells: active.map(
    (count, offset): RetentionCell => ({
      start: (week + offset) * WEEK,
      end: (week + offset + 1) * WEEK,
      active: count,
      rate: rate(count, size),
      complete: (week + offset + 1) * WEEK <= watermark,
    })
  ),
})

const result = (cohorts: CohortRow[], watermark: number): RetentionResult => ({
  cohorts,
  averages: cohorts[0]?.cells.map(() => ({ t: 'unavailable', reason: 'no_denominator' }) as const) ?? [],
  coverage: { with_identity: 1, eligible_events: 1 },
  watermark,
})

describe('heatStep', () => {
  it('splits rates into equal parts of the highest and keeps none apart', () => {
    expect(heatStep(null, 0.5)).toBe(0)
    expect(heatStep(0, 0.5)).toBe(0)
    expect(heatStep(0.001, 0.5)).toBe(1)
    expect(heatStep(0.25, 0.5)).toBe(3)
    expect(heatStep(0.5, 0.5)).toBe(HEAT_STEPS)
  })

  it('mixes evenly spaced shares of the accent', () => {
    expect(heatFill(0)).toBe('var(--fill)')
    expect(heatFill(1)).toBe('color-mix(in oklab, var(--accent) calc(var(--heat-max) * 0.2), var(--card))')
    expect(heatFill(HEAT_STEPS)).toBe('color-mix(in oklab, var(--accent) calc(var(--heat-max) * 1), var(--card))')
  })
})

describe('heatmap', () => {
  it('drops offset 0 and cells that have not started, which makes the triangle', () => {
    const watermark = 3.5 * WEEK
    const grid = heatmap(
      result([cohort(0, 10, [10, 4, 2, 1], watermark), cohort(1, 20, [20, 5, 3, 0], watermark), cohort(3, 0, [0, 0, 0, 0], watermark)], watermark)
    )
    expect(grid.columns).toBe(3)
    expect(grid.rows.map((row) => row.cells.map((cell) => cell.offset))).toEqual([[1, 2, 3], [1, 2], []])
    expect(grid.max).toBe(0.4)
    expect(grid.rows[0]?.cells[0]).toMatchObject({ rate: 0.4, step: HEAT_STEPS, complete: true })
    expect(grid.rows[0]?.cells[2]).toMatchObject({ complete: false })
  })
})

describe('trend', () => {
  it('compares the newer half of measured cohorts with the older, pooled', () => {
    const watermark = 10 * WEEK
    const cohorts = [cohort(0, 100, [100, 10], watermark), cohort(1, 100, [100, 10], watermark), cohort(2, 50, [50, 6], watermark), cohort(3, 50, [50, 6], watermark)]
    const found = trend(result(cohorts, watermark), 1)
    expect(found?.since).toBe('2026-01-03')
    expect(found?.change).toBeCloseTo(0.2)
  })

  it('needs two cohorts with a complete interval', () => {
    const watermark = 1.5 * WEEK
    expect(trend(result([cohort(0, 100, [100, 30], watermark), cohort(1, 100, [100, 20], watermark)], watermark), 1)).toBeNull()
  })

  it('says nothing about a handful of people', () => {
    const watermark = 10 * WEEK
    expect(trend(result([cohort(0, 50, [50, 5], watermark), cohort(1, 4, [4, 3], watermark)], watermark), 1)).toBeNull()
  })
})

describe('curvePoints', () => {
  it('spreads offsets across the width, skips gaps and caps at the top', () => {
    const points = curvePoints(
      [
        { offset: 1, rate: 0.5 },
        { offset: 2, rate: null },
        { offset: 3, rate: 0.75 },
      ],
      3,
      0.5,
      100,
      40
    )
    expect(points).toEqual([
      { x: 0, y: 0 },
      { x: 100, y: 0 },
    ])
  })
})

describe('moveInGrid', () => {
  const lengths = [3, 2, 0, 1]

  it('moves within a row and stops at its ends', () => {
    expect(moveInGrid(lengths, { row: 0, column: 1 }, 'ArrowRight')).toEqual({ row: 0, column: 2 })
    expect(moveInGrid(lengths, { row: 0, column: 2 }, 'ArrowRight')).toBeNull()
    expect(moveInGrid(lengths, { row: 0, column: 2 }, 'Home')).toEqual({ row: 0, column: 0 })
    expect(moveInGrid(lengths, { row: 1, column: 0 }, 'End')).toEqual({ row: 1, column: 1 })
  })

  it('keeps the column across rows, clamps to shorter rows and skips empty ones', () => {
    expect(moveInGrid(lengths, { row: 0, column: 2 }, 'ArrowDown')).toEqual({ row: 1, column: 1 })
    expect(moveInGrid(lengths, { row: 1, column: 1 }, 'ArrowDown')).toEqual({ row: 3, column: 0 })
    expect(moveInGrid(lengths, { row: 3, column: 0 }, 'ArrowUp')).toEqual({ row: 1, column: 0 })
    expect(moveInGrid(lengths, { row: 3, column: 0 }, 'ArrowDown')).toBeNull()
  })
})
