import type { ExploreResult, Metric } from '~/generated'

import { pageviewPace, type SitePace } from './site-pace'

/**
 * Today so far against a typical weekday as running totals, for the overview's pace curve: the
 * measured line up to now, the typical day's line over the whole day and the server's projection
 * of the day's end as a continuation. Plain data in milliseconds and counts; the component scales
 * it. The projection's path between now and the end of the day follows the typical day's shape,
 * so the only projected figures are the server's end-of-day estimate and its range.
 */

/** An instant and the running total there. */
export interface CurvePoint {
  at: number
  value: number
}

export interface PaceHour {
  start: number
  end: number
  /** Local hour of day, as the `hour` field reads it. */
  hour: number
  /** Counted in the hour; null for hours still to come. */
  count: number | null
  /** The typical weekday's count of the hour; null without one. */
  typical: number | null
  /** Running total at the end of the hour, or at now for the hour that holds it. */
  total: number | null
  /** The typical weekday's running total at the end of the hour. */
  typicalTotal: number | null
}

export interface DayEstimate {
  value: number
  low: number
  high: number
}

export interface DayPace {
  start: number
  end: number
  /** The instant counted up to, inside the day. */
  now: number
  hours: PaceHour[]
  /** Counted so far. */
  today: number
  /** The typical weekday up to the same time; null without enough history. */
  typicalSoFar: number | null
  /** The typical weekday's whole day; null without enough history. */
  typicalDay: number | null
  /** The server's estimate of the day's end; null without enough history or after the day. */
  projection: DayEstimate | null
  pace: SitePace
}

export interface PaceCurves {
  today: CurvePoint[]
  typical: CurvePoint[]
  /** From now to the end of the day; empty without a projection. */
  projection: CurvePoint[]
  /** The projection's range as a closed outline: low forward, high back. */
  band: CurvePoint[]
}

const count = (metric: Metric | undefined): number | null => (metric?.t === 'count' ? metric.v : null)

/** The pace of one count measure of a result for a single day by the hour, read at `at`. */
export function dayPace(result: Pick<ExploreResult, 'resolved' | 'main'>, measure: number, at: number): DayPace | null {
  const points = result.main.series?.points
  if (!points?.length) return null
  const { start, end } = result.resolved
  const now = Math.min(Math.max(at, start), end)

  let total = 0
  let typicalTotal: number | null = 0
  const hours = points.map((point, index): PaceHour => {
    const hourEnd = points[index + 1]?.start ?? end
    const counted = point.start <= now ? (count(point.values[measure]) ?? 0) : null
    const typical = count(point.compare?.[measure]?.previous)
    if (counted !== null) total += counted
    typicalTotal = typical === null || typicalTotal === null ? null : typicalTotal + typical
    return {
      start: point.start,
      end: hourEnd,
      hour: Number(point.local.slice(11, 13)),
      count: counted,
      typical,
      total: counted === null ? null : total,
      typicalTotal,
    }
  })

  const comparison = result.main.compare?.[measure]
  const estimate = result.main.projection?.[measure]
  const today = count(result.main.totals[measure]) ?? total
  const typicalSoFar = count(comparison?.previous)
  return {
    start,
    end,
    now,
    hours,
    today,
    typicalSoFar,
    typicalDay: typicalTotal,
    projection: estimate?.t === 'estimate' ? { value: estimate.value, low: estimate.low, high: estimate.high } : null,
    pace:
      comparison?.reason === 'insufficient_history'
        ? { kind: 'new' }
        : pageviewPace(today, typicalSoFar, comparison?.relative_change ?? null),
  }
}

/** The running total of a curve at `at`, linear between its points. */
export function valueAt(curve: CurvePoint[], at: number): number {
  const after = curve.findIndex((point) => point.at >= at)
  if (after < 0) return curve.at(-1)?.value ?? 0
  const next = curve[after]
  const previous = curve[after - 1]
  if (!next) return 0
  if (!previous || next.at === previous.at) return next.value
  return previous.value + ((next.value - previous.value) * (at - previous.at)) / (next.at - previous.at)
}

export function paceCurves(pace: DayPace): PaceCurves {
  const today: CurvePoint[] = [{ at: pace.start, value: 0 }]
  const typical: CurvePoint[] = [{ at: pace.start, value: 0 }]
  for (const hour of pace.hours) {
    if (hour.total !== null) today.push({ at: Math.min(hour.end, pace.now), value: hour.total })
    if (hour.typicalTotal !== null) typical.push({ at: hour.end, value: hour.typicalTotal })
  }
  if (pace.typicalDay === null) typical.length = 1

  const estimate = pace.projection
  if (!estimate || pace.now >= pace.end) return { today, typical: typical.length > 1 ? typical : [], projection: [], band: [] }

  // The share of the typical day's remainder reached by each hour end shapes the way to the estimate.
  const typicalNow = typical.length > 1 ? valueAt(typical, pace.now) : 0
  const remainder = (pace.typicalDay ?? 0) - typicalNow
  const share = (at: number) =>
    remainder > 0 ? (valueAt(typical, at) - typicalNow) / remainder : (at - pace.now) / (pace.end - pace.now)
  const ends = [pace.now, ...pace.hours.filter((hour) => hour.end > pace.now).map((hour) => hour.end)]
  const toward = (target: number) => ends.map((at) => ({ at, value: pace.today + share(at) * (target - pace.today) }))
  const low = toward(estimate.low)
  const high = toward(estimate.high)
  return {
    today,
    typical: typical.length > 1 ? typical : [],
    projection: toward(estimate.value),
    band: [...low, ...high.reverse()],
  }
}

/** The hour holding `at`, or the last one before it. */
export function hourIndexAt(pace: DayPace, at: number): number {
  const index = pace.hours.findIndex((hour) => at < hour.end)
  return index < 0 ? pace.hours.length - 1 : index
}

/** An SVG path through `points`, scaled by `x` and `y`; `close` ends it as an outline. */
export function svgPath(points: CurvePoint[], x: (at: number) => number, y: (value: number) => number, close = false): string {
  if (points.length === 0) return ''
  const path = points.map((point, index) => `${index === 0 ? 'M' : 'L'}${x(point.at).toFixed(1)} ${y(point.value).toFixed(1)}`).join(' ')
  return close ? `${path} Z` : path
}

/**
 * The value axis: at most four round gridlines, the top one at or above everything drawn.
 * 9,100 gets 2,500 to 10,000.
 */
export function paceAxis(pace: DayPace): { max: number; ticks: number[] } {
  const top = Math.max(pace.today, pace.typicalDay ?? 0, pace.projection?.high ?? 0, 4)
  const magnitude = 10 ** Math.floor(Math.log10(top / 4))
  const step = [1, 2, 2.5, 5, 10].map((factor) => factor * magnitude).find((candidate) => top / candidate <= 4) ?? 10 * magnitude
  const count = Math.ceil(top / step)
  return { max: count * step, ticks: Array.from({ length: count }, (_, index) => (index + 1) * step) }
}
