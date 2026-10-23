import { describe, expect, it } from 'vitest'

import type { ExploreResult, Projection } from '~/generated'
import { dayPace, hourIndexAt, paceAxis, paceCurves, svgPath, valueAt } from '~/lib/day-pace'

const HOUR = 3_600_000
const START = Date.UTC(2026, 9, 18, 0)

/** A four-hour "day" read at 02:30, typical 10 per hour. */
const result = (
  counts: number[],
  patch: { typical?: (number | null)[]; projection?: Projection; reason?: 'insufficient_history' } = {}
): Pick<ExploreResult, 'resolved' | 'main'> => {
  const typical = patch.typical ?? counts.map(() => 10)
  const today = counts.slice(0, 3).reduce((sum, value) => sum + value, 0)
  return {
    resolved: { start: START, end: START + 4 * HOUR, timezone: 'UTC' },
    main: {
      cut: { measures: ['pageviews'], dimensions: [], sort: [], page: { size: 1, index: 0 } },
      totals: [{ t: 'count', v: today }],
      compare: [
        patch.reason
          ? { previous: { t: 'unavailable', reason: patch.reason }, reason: patch.reason }
          : { previous: { t: 'count', v: 25 }, relative_change: (today - 25) / 25 },
      ],
      projection: [patch.projection ?? { t: 'estimate', value: 80, low: 70, high: 90, basis_days: 8 }],
      rows: [],
      row_count: 0,
      identity_coverage: { eligible_events: 0, with_identity: 0 },
      series: {
        granularity: 'hour',
        series_sum_differs: false,
        points: counts.map((value, index) => ({
          start: START + index * HOUR,
          local: `2026-10-18T0${index}:00`,
          values: [{ t: 'count', v: value }],
          compare: [{ previous: typical[index] === null ? { t: 'unavailable', reason: 'insufficient_history' } : { t: 'count', v: typical[index] ?? 0 } }],
        })),
      },
    },
  }
}

const AT = START + 2.5 * HOUR

describe('dayPace', () => {
  it('runs totals up to now and leaves later hours uncounted', () => {
    const pace = dayPace(result([10, 20, 6, 0]), 0, AT)
    expect(pace?.hours.map((hour) => [hour.hour, hour.count, hour.total, hour.typicalTotal])).toEqual([
      [0, 10, 10, 10],
      [1, 20, 30, 20],
      [2, 6, 36, 30],
      [3, null, null, 40],
    ])
    expect(pace).toMatchObject({ today: 36, typicalSoFar: 25, typicalDay: 40, projection: { value: 80, low: 70, high: 90 } })
  })

  it('reads the pace sentence the way the site chip does', () => {
    expect(dayPace(result([10, 20, 6, 0]), 0, AT)?.pace).toEqual({ kind: 'quiet', views: 36 })
    expect(dayPace(result([100, 200, 60, 0]), 0, AT)?.pace).toMatchObject({ kind: 'above' })
    expect(dayPace(result([1, 1, 1, 0], { reason: 'insufficient_history' }), 0, AT)?.pace).toEqual({ kind: 'new' })
  })

  it('has no typical day when an hour has no typical value', () => {
    expect(dayPace(result([1, 1, 1, 0], { typical: [10, null, 10, 10] }), 0, AT)?.typicalDay).toBeNull()
  })
})

describe('paceCurves', () => {
  it('ends the measured line at now and leads the projection along the typical shape to the estimate', () => {
    const pace = dayPace(result([10, 20, 6, 0], { typical: [10, 10, 20, 20] }), 0, AT)
    if (!pace) throw new Error('no pace')
    const curves = paceCurves(pace)
    expect(curves.today.at(-1)).toEqual({ at: AT, value: 36 })
    expect(curves.typical.map((point) => point.value)).toEqual([0, 10, 20, 40, 60])
    // Typical at now is 30 of 60: hour 3's end is a third of the remainder, the day's end all of it.
    expect(curves.projection.map((point) => Math.round(point.value))).toEqual([36, 51, 80])
    expect(curves.band).toHaveLength(6)
    expect(curves.band.at(2)?.value).toBe(70)
    expect(curves.band.at(3)?.value).toBe(90)
  })

  it('draws no projection without an estimate', () => {
    const pace = dayPace(result([10, 20, 6, 0], { projection: { t: 'unavailable', reason: 'insufficient_history' } }), 0, AT)
    if (!pace) throw new Error('no pace')
    expect(paceCurves(pace)).toMatchObject({ projection: [], band: [] })
  })
})

describe('helpers', () => {
  it('interpolates a curve and finds the hour of an instant', () => {
    const curve = [
      { at: 0, value: 0 },
      { at: 10, value: 100 },
    ]
    expect(valueAt(curve, 2.5)).toBe(25)
    expect(valueAt(curve, 20)).toBe(100)
    const pace = dayPace(result([1, 1, 1, 0]), 0, AT)
    if (!pace) throw new Error('no pace')
    expect(hourIndexAt(pace, START + 1.2 * HOUR)).toBe(1)
    expect(hourIndexAt(pace, START + 9 * HOUR)).toBe(3)
  })

  it('picks round gridlines above everything drawn', () => {
    const pace = dayPace(result([1, 1, 1, 0], { projection: { t: 'estimate', value: 8_500, low: 8_000, high: 9_100, basis_days: 8 } }), 0, AT)
    if (!pace) throw new Error('no pace')
    expect(paceAxis(pace)).toEqual({ max: 10_000, ticks: [2_500, 5_000, 7_500, 10_000] })
  })

  it('writes scaled paths', () => {
    const points = [
      { at: 0, value: 0 },
      { at: 1, value: 2 },
    ]
    expect(svgPath(points, (at) => at * 10, (value) => 100 - value, true)).toBe('M0.0 100.0 L10.0 98.0 Z')
  })
})
