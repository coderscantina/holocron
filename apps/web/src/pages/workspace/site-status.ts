import type { SiteSummary, Summary } from '~/generated'
import { groupByBaseDomain, type DomainGroup } from '~/lib/base-domain'

/** Above this many sites the status list groups by base domain; at or below it stays flat. */
export const GROUPING_THRESHOLD = 8

export type StatusSite = Pick<SiteSummary, 'id' | 'name' | 'domain'>

/** A fresh summary says how the site is doing; a stale or missing one has nothing to rank by. */
const fresh = (summary: Summary | undefined) => (summary && !summary.stale ? summary : null)

/** How far today is from a typical day, in either direction. 0 where there is no comparison. */
const distance = (summary: Summary) => Math.abs(summary.pageviews.relative_change ?? 0)

/** Sites with findings first, then the largest move from typical, then by name. Sites without a fresh summary come last. */
export function sortSites<T extends StatusSite>(sites: readonly T[], summaries: ReadonlyMap<string, Summary>): T[] {
  const rank = (site: T) => {
    const summary = fresh(summaries.get(site.id))
    return summary ? { known: 1, findings: summary.finding_count, distance: distance(summary) } : { known: 0, findings: 0, distance: 0 }
  }
  return [...sites].sort((a, b) => {
    const left = rank(a)
    const right = rank(b)
    return (
      right.known - left.known ||
      right.findings - left.findings ||
      right.distance - left.distance ||
      a.name.localeCompare(b.name) ||
      a.id.localeCompare(b.id)
    )
  })
}

/** The sites in display order: one flat list, or groups by base domain when there are many. */
export function arrangeSites<T extends StatusSite>(sites: readonly T[], summaries: ReadonlyMap<string, Summary>): DomainGroup<T>[] {
  const sorted = sortSites(sites, summaries)
  return sorted.length > GROUPING_THRESHOLD ? groupByBaseDomain(sorted) : [{ baseDomain: '', sites: sorted }]
}

/** How many sites have at least one finding to look at. */
export function needingAttention(sites: readonly StatusSite[], summaries: ReadonlyMap<string, Summary>): number {
  return sites.filter((site) => (fresh(summaries.get(site.id))?.finding_count ?? 0) > 0).length
}

export interface SparklineGeometry {
  today: string
  area: string
  typical: string[]
}

const WIDTH = 96
const HEIGHT = 24
const PAD = 2

/**
 * Point lists for a 96 by 24 box. Today runs up to the last hour with traffic (later hours are
 * zero because they have not happened); typical runs the whole day, broken where it has no value.
 * Both share one vertical scale so the two lines compare.
 */
export function sparkline(summary: Summary): SparklineGeometry {
  const { hourly_today: today, hourly_typical: typical } = summary
  const hours = Math.max(today.length, typical.length, 2)
  const top = Math.max(1, ...today, ...typical.map((value) => value ?? 0))
  const x = (hour: number) => (hour / (hours - 1)) * WIDTH
  const y = (value: number) => HEIGHT - PAD - (value / top) * (HEIGHT - 2 * PAD)
  const point = (hour: number, value: number) => `${x(hour).toFixed(1)},${y(value).toFixed(1)}`

  const last = today.findLastIndex((value) => value > 0)
  const measured = today.slice(0, Math.max(last, 0) + 1).map((value, hour) => point(hour, value))
  const area = measured.length > 1 ? `${x(0).toFixed(1)},${HEIGHT} ${measured.join(' ')} ${x(measured.length - 1).toFixed(1)},${HEIGHT}` : ''

  const runs: string[][] = []
  typical.forEach((value, hour) => {
    if (value === null) runs.push([])
    else if (runs.length === 0) runs.push([point(hour, value)])
    else runs[runs.length - 1]?.push(point(hour, value))
  })
  return {
    today: measured.join(' '),
    area,
    typical: runs.filter((run) => run.length > 1).map((run) => run.join(' ')),
  }
}

const HOUR_MS = 3_600_000
const DAY_MS = 24 * HOUR_MS

/** How far a timezone's wall clock is ahead of UTC at an instant, in milliseconds. */
function offsetAt(ms: number, timeZone: string): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    hourCycle: 'h23',
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
  }).formatToParts(ms)
  const part = (type: Intl.DateTimeFormatPartTypes) => Number(parts.find((entry) => entry.type === type)?.value ?? 0)
  const wall = Date.UTC(part('year'), part('month') - 1, part('day'), part('hour'), part('minute'), part('second'))
  return wall - Math.floor(ms / 1000) * 1000
}

/** Hours since local midnight at an instant: the index into a summary's hourly arrays, daylight saving days included. */
export function hourOfDay(ms: number, timeZone: string): number {
  const offset = offsetAt(ms, timeZone)
  const midnightWall = Math.floor((ms + offset) / DAY_MS) * DAY_MS
  const midnight = midnightWall - offsetAt(midnightWall - offset, timeZone)
  return Math.floor((ms - midnight) / HOUR_MS)
}

/** Whether a fresh summary counted pageviews in the hour it was computed in. */
export function activeThisHour(summary: Summary | undefined): boolean {
  const current = fresh(summary)
  return !!current && (current.hourly_today[hourOfDay(current.computed_at, current.timezone)] ?? 0) > 0
}
