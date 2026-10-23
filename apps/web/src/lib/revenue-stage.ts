import { COUNTRY_CENTROIDS, type Metric, type Row } from '~/generated'

/**
 * Shapes of the revenue stage: per-currency cuts (currency first, then hour, country, city or
 * channel) narrowed to one currency, the 24 hours of the day with the busiest stretch, and the
 * countries placed on the globe at their centroids, sized by their share of the largest.
 */

/** A row's amount in `code`'s minor units, 0 when the row has none in it. */
export function unitsIn(metric: Metric | undefined, code: string): number {
  if (metric?.t !== 'money') return 0
  const money = metric.v.find((candidate) => candidate.currency === code)
  return money ? Number(money.minor_units) : 0
}

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

export type RevenuePlace = { country: string; lat: number; lng: number; units: number; scale: number }

/**
 * Countries with revenue at their centroids, largest first. `scale` is the square root of the
 * share of the largest, so a disc's area follows the amount.
 */
export function placesOf(rows: readonly Row[], code: string): RevenuePlace[] {
  const placed = rowsIn(rows, code).flatMap((row) => {
    const country = row.key[0]?.t === 'string' ? row.key[0].v : null
    const centroid = country ? COUNTRY_CENTROIDS[country] : undefined
    return country && centroid ? [{ country, lat: centroid[0] / 10, lng: centroid[1] / 10, units: row.units }] : []
  })
  const max = Math.max(1, ...placed.map((place) => place.units))
  return placed.sort((a, b) => b.units - a.units).map((place) => ({ ...place, scale: Math.sqrt(place.units / max) }))
}

/** The first row's keys after the currency: the top earner of a cut sorted by revenue. */
export function topOf(rows: readonly Row[], code: string): { key: Row['key']; units: number } | null {
  return rowsIn(rows, code).sort((a, b) => b.units - a.units)[0] ?? null
}
