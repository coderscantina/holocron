import type { Cell, Metric, Projection, Row, Series } from '~/generated'

/**
 * Shapes of the revenue summary, always in one currency's minor units: the 24 hours of the day
 * with the busiest stretch, the running total against the comparison with the pace estimate
 * to the end of the range, and the sources ranked by revenue beside their share of visitors.
 */

/** A row's amount in `code`'s minor units, 0 when the row has none in it. */
export function unitsIn(metric: Metric | undefined, code: string): number {
  if (metric?.t !== 'money') return 0
  const money = metric.v.find((candidate) => candidate.currency === code)
  return money ? Number(money.minor_units) : 0
}

const countOf = (metric: Metric | undefined) => (metric?.t === 'count' ? metric.v : 0)

const text = (row: Row, index: number) => {
  const cell = row.key[index]
  return cell?.t === 'string' ? cell.v : null
}

/** The rows of a currency-first cut in `code`, each with its keys after the currency and its amount. */
export function rowsIn(rows: readonly Row[], code: string): { key: Row['key']; units: number }[] {
  return rows
    .filter((row) => text(row, 0) === code)
    .map((row) => ({ key: row.key.slice(1), units: unitsIn(row.values[0], code) }))
    .filter((row) => row.units > 0)
}

/** Revenue per local hour 0 to 23 from an hour cut. */
export function hourly(rows: readonly Row[], code: string): number[] {
  const hours = Array.from({ length: 24 }, () => 0)
  for (const row of rowsIn(rows, code)) {
    const cell = row.key[0]
    if (cell?.t === 'integer' && cell.v >= 0 && cell.v < 24) hours[cell.v] = (hours[cell.v] ?? 0) + row.units
  }
  return hours
}

export type Stretch = { start: number; hours: number; share: number }

/** The `length` consecutive hours, wrapping past midnight, holding the most revenue. `null` without any. */
export function busiest(hours: readonly number[], length = 3): Stretch | null {
  const total = hours.reduce((sum, units) => sum + units, 0)
  if (total <= 0) return null
  let best: Stretch | null = null
  for (let start = 0; start < hours.length; start++) {
    let sum = 0
    for (let offset = 0; offset < length; offset++) sum += hours[(start + offset) % hours.length] ?? 0
    if (!best || sum > best.share * total) best = { start, hours: length, share: sum / total }
  }
  return best
}

export type Band = { value: number; low: number; high: number }

/** One currency of a money projection (0015 Pace projection), `null` when there is none. */
export function bandIn(projection: Projection | null | undefined, code: string): (Band & { basisDays: number }) | null {
  if (projection?.t !== 'money_estimate') return null
  const money = projection.v.find((candidate) => candidate.currency === code)
  return money ? { value: money.value, low: money.low, high: money.high, basisDays: projection.basis_days } : null
}

export type PacePoint = {
  start: number
  local: string
  /** The running total at the end of the bucket; `null` after the bucket that holds now. */
  actual: number | null
  /** The comparison's running total; `null` without a comparison. */
  previous: number | null
  /** The estimated running total; only after the bucket that holds now. */
  estimate: Band | null
}

export type Pace = {
  points: PacePoint[]
  /** The running total so far, and the comparison's at the same bucket. */
  actual: number
  previous: number | null
  /** Where the range ends, as the server projected it. */
  estimate: (Band & { basisDays: number }) | null
}

/**
 * The running total of one money measure against the comparison's running total. With a
 * projection, the estimate runs from now to the server's end-of-range estimate and band,
 * shaped by the projected buckets so the curve follows the usual rhythm of the remaining time.
 */
export function pace(
  series: Series,
  measure: number,
  code: string,
  now: number,
  projected?: { series: Series | undefined; total: Projection | null | undefined }
): Pace {
  const current = series.points.findLastIndex((point) => point.start <= now)
  const compared = series.points.some((point) => point.compare?.[measure])
  let actual = 0
  let previous = 0
  let previousNow: number | null = null
  const points: PacePoint[] = series.points.map((point, index) => {
    previous += unitsIn(point.compare?.[measure]?.previous, code)
    if (index <= current) actual += unitsIn(point.values[measure], code)
    if (index === current && compared) previousNow = previous
    return {
      start: point.start,
      local: point.local,
      actual: index <= current ? actual : null,
      previous: compared ? previous : null,
      estimate: null,
    }
  })

  const estimate = bandIn(projected?.total, code)
  const last = points.length - 1
  if (estimate && current >= 0 && current < last) {
    const byStart = new Map(
      (projected?.series?.projection ?? []).map((point) => [point.start, bandIn(point.values[0], code)?.value ?? 0])
    )
    const remaining = series.points.slice(current).map((point, offset) => {
      const projectedBucket = byStart.get(point.start) ?? 0
      const measured = offset === 0 ? unitsIn(point.values[measure], code) : 0
      return Math.max(0, projectedBucket - measured)
    })
    const sum = remaining.reduce((total, units) => total + units, 0)
    let reached = remaining[0] ?? 0
    for (let index = current + 1; index <= last; index++) {
      reached += remaining[index - current] ?? 0
      const share = sum > 0 ? reached / sum : (index - current) / (last - current)
      const at = (end: number) => actual + (end - actual) * share
      const point = points[index]
      if (point) point.estimate = { value: at(estimate.value), low: at(estimate.low), high: at(estimate.high) }
    }
  }
  return { points, actual, previous: previousNow, estimate }
}

/** A source pays more than its traffic from this ratio of revenue share to visitor share, less below its inverse. */
export const PAYS_MORE = 1.5
export const PAYS_LESS = 1 / PAYS_MORE

export type Source = {
  /** The source's cell; `null` for the rest taken together. */
  key: Cell | null
  /** Net revenue in minor units. */
  net: number
  visitors: number
  /** Visitors with a revenue record in this currency. */
  paying: number
  revenueShare: number
  visitorShare: number
  /** Net per visitor in minor units; `null` without visitors. */
  perVisitor: number | null
  conversion: number | null
  pays: 'more' | 'less' | 'even'
}

/**
 * The sources of the most revenue in `code`, then the rest as one row. `paying` is a
 * currency-first cut with net, records and visitors over revenue records only; `all` has the
 * same source's visitors over all traffic. Both are counted the way the cut's visitor measure
 * counts, so shares and rates compare like with like.
 */
export function traffic(
  paying: readonly Row[],
  all: readonly Row[],
  code: string,
  totals: { net: number; visitors: number },
  limit = 7
): Source[] {
  const visitorsOf = new Map(all.map((row) => [JSON.stringify(row.key[0]), countOf(row.values[0])]))
  const ranked = paying
    .filter((row) => text(row, 0) === code)
    .map((row) => ({
      key: row.key[1] ?? null,
      net: unitsIn(row.values[0], code),
      paying: countOf(row.values[2]),
      visitors: visitorsOf.get(JSON.stringify(row.key[1])) ?? 0,
    }))
    .sort((a, b) => b.net - a.net)
  const rest = ranked.slice(limit)
  const rows =
    rest.length > 0
      ? [
          ...ranked.slice(0, limit),
          {
            key: null,
            net: rest.reduce((sum, row) => sum + row.net, 0),
            paying: rest.reduce((sum, row) => sum + row.paying, 0),
            visitors: rest.reduce((sum, row) => sum + row.visitors, 0),
          },
        ]
      : ranked
  return rows.map((row) => {
    const revenueShare = totals.net > 0 ? row.net / totals.net : 0
    const visitorShare = totals.visitors > 0 ? row.visitors / totals.visitors : 0
    const ratio = visitorShare > 0 ? revenueShare / visitorShare : 1
    return {
      ...row,
      revenueShare,
      visitorShare,
      perVisitor: row.visitors > 0 ? row.net / row.visitors : null,
      conversion: row.visitors > 0 ? row.paying / row.visitors : null,
      pays: ratio >= PAYS_MORE ? 'more' : ratio <= PAYS_LESS ? 'less' : 'even',
    }
  })
}

/** A source too small to stand for the site is not named as its best payer. */
const NAMED_SHARE_MIN = 0.05

/** The named source paying the most per visitor, when it pays at least 1.5 times the site's rate. */
export function bestPayer(sources: readonly Source[], siteRate: number): Source | null {
  const candidates = sources.filter(
    (source) => source.key !== null && source.visitorShare >= NAMED_SHARE_MIN && (source.perVisitor ?? 0) >= siteRate * PAYS_MORE
  )
  return candidates.reduce<Source | null>((best, source) => ((source.perVisitor ?? 0) > (best?.perVisitor ?? 0) ? source : best), null)
}
