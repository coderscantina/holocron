import type { ExploreResult, Granularity, Metric, Projection, Series } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'
import type { Baseline } from '~/services/exploration'

/**
 * What the trend chart draws besides measured values, as plain data: projected buckets, the
 * baseline, the Now marker and the consequence sentences (design contract, Trend chart). Nothing
 * here formats or translates; `TrendChart.vue` fills the messages.
 */

export type Estimate = Extract<Projection, { t: 'estimate' }>

/** Where the chart's present is. */
export interface ChartClock {
  /** The application clock, Unix milliseconds. */
  now: number
  /** End of the range, Unix milliseconds, exclusive. */
  end: number
  /** The analytical watermark, only while analytics is stale: the last instant counted exactly. */
  watermark?: number
}

/** The chart clock of a result read at `now`: the watermark joins only while analytics is stale. */
export const chartClock = (now: number, { resolved, freshness }: Pick<ExploreResult, 'resolved' | 'freshness'>): ChartClock =>
  freshness.stale ? { now, end: resolved.end, watermark: freshness.watermark_at } : { now, end: resolved.end }

export interface Bucket {
  start: number
  local: string
  /** The measured value, 0 when unavailable. */
  value: number
  /** Starts after the bucket that holds now, so nothing in it is measured yet. */
  future: boolean
  estimate: Estimate | null
  baseline: Metric | undefined
}

export const numeric = (metric: Metric | undefined): number => {
  if (!metric) return 0
  switch (metric.t) {
    case 'count':
    case 'millis':
    case 'ratio':
      return metric.v
    case 'decimal':
      return Number(metric.v)
    case 'money':
      return metric.v.length === 1 ? Number(metric.v[0]?.amount) : 0
    case 'unavailable':
      return 0
  }
}

/**
 * The buckets of one measure. The server projects every bucket that ends after its now, so the
 * first projected bucket holds now and the ones after it are still to come.
 */
export function buckets(series: Series, measure: number): Bucket[] {
  const projected = new Map((series.projection ?? []).map((point) => [point.start, point.values[measure] ?? null]))
  const first = series.points.findIndex((point) => projected.has(point.start))
  return series.points.map((point, index) => {
    const projection = projected.get(point.start)
    return {
      start: point.start,
      local: point.local,
      value: numeric(point.values[measure]),
      future: first >= 0 && index > first,
      estimate: projection?.t === 'estimate' ? projection : null,
      baseline: point.compare?.[measure]?.previous,
    }
  })
}

/** The bucket holding an instant, or -1 outside `[first start, end)`. */
export function bucketOf(points: readonly { start: number }[], at: number, end: number): number {
  const first = points[0]
  if (!first || at < first.start || at >= end) return -1
  return points.findLastIndex((point) => point.start <= at)
}

/** A local wall time `YYYY-MM-DDTHH:MM` moved by a duration. */
export const shiftLocal = (local: string, ms: number) =>
  new Date(Date.parse(`${local}:00Z`) + ms).toISOString().slice(0, 16)

export interface NowMarker {
  at: number
  /** Local wall time of `at`. */
  local: string
  /** At the watermark rather than the clock. */
  stale: boolean
  /** Horizontal position as a fraction of the plot. */
  position: number
}

/**
 * The Now marker: at the watermark while analytics is stale, otherwise at the clock, and only
 * when that instant lies inside the range.
 */
export function nowMarker(points: { start: number; local: string }[], clock: ChartClock): NowMarker | null {
  const stale = clock.watermark !== undefined
  const at = clock.watermark ?? clock.now
  const first = points[0]
  if (!first || at < first.start || at >= clock.end) return null
  const index = points.findLastIndex((point) => point.start <= at)
  const point = points[index] ?? first
  const next = points[index + 1]?.start ?? clock.end
  return {
    at,
    local: shiftLocal(point.local, at - point.start),
    stale,
    position: (index + (at - point.start) / (next - point.start)) / points.length,
  }
}

export type Consequence = { kind: 'passes'; value: number; at: string } | { kind: 'busiest'; start: string; end: string }

const HOUR = 3_600_000
const DAY = 86_400_000
const HALF_HOUR = HOUR / 2

/** The next round number above a positive value: 3,630 passes 4,000 next. */
export const nextRound = (value: number) => {
  const magnitude = 10 ** Math.floor(Math.log10(value))
  return (Math.floor(value / magnitude) + 1) * magnitude
}

/**
 * Up to two sentences on what the estimate implies, for hour and day buckets: when the running
 * total passes the next round number, and the busiest bucket still to come when it beats every
 * bucket so far. A visitor series that does not add up to its total gets no running total.
 * `at` is the Now marker's instant; without one the current bucket counts from its start.
 */
export function consequences(
  rows: Bucket[],
  granularity: Granularity,
  options: { at: number | null; sumDiffers: boolean }
): Consequence[] {
  if (granularity !== 'hour' && granularity !== 'day') return []
  const current = rows.findIndex((row) => row.estimate !== null)
  if (current < 0) return []
  const step = granularity === 'hour' ? HOUR : DAY
  const out: Consequence[] = []

  const soFar = rows.reduce((sum, row) => (row.future ? sum : sum + row.value), 0)
  if (!options.sumDiffers && soFar > 0) {
    const target = nextRound(soFar)
    let total = soFar
    for (const [index, row] of rows.entries()) {
      if (index < current || !row.estimate) continue
      const remaining = index === current ? Math.max(0, row.estimate.value - row.value) : row.estimate.value
      if (total + remaining < target) {
        total += remaining
        continue
      }
      const from = index === current && options.at !== null ? Math.max(row.start, options.at) : row.start
      const end = rows[index + 1]?.start ?? row.start + step
      const instant = from + ((target - total) / remaining) * (end - from)
      const at =
        granularity === 'hour'
          ? shiftLocal(row.local, Math.ceil((instant - row.start) / HALF_HOUR) * HALF_HOUR)
          : row.local
      out.push({ kind: 'passes', value: target, at })
      break
    }
  }

  const ahead = rows.filter((row) => row.future && row.estimate)
  const busiest = ahead.reduce<Bucket | null>(
    (best, row) => (best === null || (row.estimate?.value ?? 0) > (best.estimate?.value ?? 0) ? row : best),
    null
  )
  const behind = rows.slice(0, current + 1).map((row, index) => (index === current ? (row.estimate?.value ?? row.value) : row.value))
  if (busiest?.estimate && busiest.estimate.value > Math.max(0, ...behind)) {
    const index = rows.indexOf(busiest)
    out.push({ kind: 'busiest', start: busiest.local, end: rows[index + 1]?.local ?? shiftLocal(busiest.local, step) })
  }
  return out
}

/** The basis of an estimate: the same weekday when the range is one day, else baseline days. */
export const basisKey = (baseline: Baseline | null): MessageKey =>
  baseline?.kind === 'weekday' ? `explore.chart.basis.weekday.${baseline.weekday}` : 'explore.chart.basis.days'

/** What the baseline line is labelled with. */
export const baselineKey = (baseline: Baseline): MessageKey => {
  switch (baseline.kind) {
    case 'weekday':
      return 'explore.scope.compare.options.weekday'
    case 'weekdays':
      return 'explore.scope.compare.options.weekdays'
    case 'previous':
      return 'explore.scope.compare.options.previous'
  }
}

/** A message whose placeholders are all numbers, formatted by the caller. */
export interface Phrase {
  key: MessageKey
  named: Record<string, number>
}

export interface BucketText {
  /** Whether the bucket has a measured value to show; not for buckets still to come. */
  measured: boolean
  /** "Estimate 236, between 220 and 250". */
  estimate: Phrase | null
  /** "The estimate comes from the last 8 Tuesdays." */
  basis: Phrase | null
}

/** What the tooltip and the accessible table say about one bucket, so both agree. */
export function bucketText(bucket: Bucket, baseline: Baseline | null): BucketText {
  const { estimate } = bucket
  return {
    measured: !bucket.future,
    estimate: estimate
      ? {
          key: 'explore.chart.estimate',
          named: { value: Math.round(estimate.value), low: Math.round(estimate.low), high: Math.round(estimate.high) },
        }
      : null,
    basis: estimate ? { key: basisKey(baseline), named: { count: estimate.basis_days } } : null,
  }
}
