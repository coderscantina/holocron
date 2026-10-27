/** Pure parts of the shadow comparison: days in a timezone, tolerances and top lists. */

export const TOLERANCE = {
  /** Pageviews per day: relative difference. */
  pageviews: 0.01,
  /** Visitors per day: the mirror drops Umami's cache token, so sessions differ more. */
  visitors: 0.05,
  /** Daily counts this close in absolute terms always pass, so a day with 4 views is not noise. */
  absoluteFloor: 2,
  /** Share of the top 10 that both sides must have in common. */
  topOverlap: 0.7,
  /** Share of common entries whose rank differs by at most `rankSlack`. */
  topRank: 0.7,
  rankSlack: 2,
} as const

export const TOP_SIZE = 10
export const MAX_DAYS = 366

const DAY = /^\d{4}-\d{2}-\d{2}$/

export function isDay(value: string): boolean {
  if (!DAY.test(value)) return false
  const date = new Date(`${value}T00:00:00Z`)
  return !Number.isNaN(date.getTime()) && date.toISOString().startsWith(value)
}

export function addDays(day: string, n: number): string {
  const date = new Date(`${day}T00:00:00Z`)
  date.setUTCDate(date.getUTCDate() + n)
  return date.toISOString().slice(0, 10)
}

/** Every date from `from` to `to`, inclusive. */
export function daysBetween(from: string, to: string): string[] {
  const days: string[] = []
  for (let day = from; day <= to; day = addDays(day, 1)) days.push(day)
  return days
}

export function isTimezone(name: string): boolean {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone: name })
    return true
  } catch {
    return false
  }
}

/** The local calendar date of an instant in a timezone. */
export function localDay(ms: number, timezone: string): string {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(ms)
  return parts
}

/** Offset of a timezone from UTC at an instant, in milliseconds. */
function offsetAt(ms: number, timezone: string): number {
  const parts = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', {
      timeZone: timezone,
      hourCycle: 'h23',
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
      second: 'numeric',
    })
      .formatToParts(ms)
      .map((p) => [p.type, Number(p.value)]),
  ) as Record<string, number>
  const asUtc = Date.UTC(parts.year!, parts.month! - 1, parts.day!, parts.hour!, parts.minute!, parts.second!)
  return asUtc - Math.floor(ms / 1000) * 1000
}

/** The UTC instant at which a local date starts in a timezone. */
export function dayStart(day: string, timezone: string): number {
  const wall = Date.parse(`${day}T00:00:00Z`)
  const first = wall - offsetAt(wall, timezone)
  return wall - offsetAt(first, timezone)
}

/** UTC bounds `[start, end)` of a local day, so a 23 or 25 hour day on a DST change is exact. */
export function dayBounds(day: string, timezone: string): { start: number; end: number } {
  return { start: dayStart(day, timezone), end: dayStart(addDays(day, 1), timezone) }
}

export type Verdict = 'ok' | 'differs'

/** Whether two daily counts agree within a relative tolerance or the absolute floor. */
export function countVerdict(umami: number, holocron: number, tolerance: number): Verdict {
  const diff = Math.abs(umami - holocron)
  if (diff <= TOLERANCE.absoluteFloor) return 'ok'
  return diff / Math.max(umami, holocron) <= tolerance ? 'ok' : 'differs'
}

/** Relative difference of Holocron to Umami in percent, for display. */
export function percentDiff(umami: number, holocron: number): number | null {
  if (umami === 0) return holocron === 0 ? 0 : null
  return ((holocron - umami) / umami) * 100
}

export interface TopComparison {
  overlap: number
  rankAgreement: number
  verdict: Verdict
  onlyUmami: string[]
  onlyHolocron: string[]
}

/** Compares two ranked top lists by the entries they share and where those entries sit. */
export function compareTop(umami: string[], holocron: string[]): TopComparison {
  const u = umami.slice(0, TOP_SIZE)
  const h = holocron.slice(0, TOP_SIZE)
  const inHolocron = new Set(h)
  const inUmami = new Set(u)
  const common = u.filter((key) => inHolocron.has(key))
  const size = Math.max(u.length, h.length)
  const overlap = size === 0 ? 1 : common.length / size
  const near = common.filter((key) => Math.abs(u.indexOf(key) - h.indexOf(key)) <= TOLERANCE.rankSlack)
  const rankAgreement = common.length === 0 ? (size === 0 ? 1 : 0) : near.length / common.length
  return {
    overlap,
    rankAgreement,
    verdict: overlap >= TOLERANCE.topOverlap && rankAgreement >= TOLERANCE.topRank ? 'ok' : 'differs',
    onlyUmami: u.filter((key) => !inHolocron.has(key)),
    onlyHolocron: h.filter((key) => !inUmami.has(key)),
  }
}

export interface DayCounts {
  pageviews: number
  visitors: number
}

export interface DayRow {
  day: string
  umami: DayCounts
  holocron: DayCounts
  pageviews: Verdict
  visitors: Verdict
}

export interface Outage {
  day: string
  umami: DayCounts
  holocron: DayCounts
  /** `both` is quiet traffic on both sides; the others mean one side missed a day the other saw. */
  zero: 'umami' | 'holocron' | 'both'
}

/** Splits days into compared days and possible outages (a side with no pageviews at all). */
export function classifyDays(
  days: string[],
  umami: Map<string, DayCounts>,
  holocron: Map<string, DayCounts>,
): { rows: DayRow[]; outages: Outage[] } {
  const none: DayCounts = { pageviews: 0, visitors: 0 }
  const rows: DayRow[] = []
  const outages: Outage[] = []
  for (const day of days) {
    const u = umami.get(day) ?? none
    const h = holocron.get(day) ?? none
    if (u.pageviews === 0 || h.pageviews === 0) {
      const zero = u.pageviews === 0 && h.pageviews === 0 ? 'both' : u.pageviews === 0 ? 'umami' : 'holocron'
      outages.push({ day, umami: u, holocron: h, zero })
      continue
    }
    rows.push({
      day,
      umami: u,
      holocron: h,
      pageviews: countVerdict(u.pageviews, h.pageviews, TOLERANCE.pageviews),
      visitors: countVerdict(u.visitors, h.visitors, TOLERANCE.visitors),
    })
  }
  return { rows, outages }
}

export interface SiteReport {
  site: string
  name: string
  timezone: string
  days: DayRow[]
  outages: Outage[]
  pages: TopComparison
  countries: TopComparison
  problems: string[]
}

/** The reasons a site is off, empty when it is within tolerance. Days with one silent side count. */
export function siteProblems(report: Pick<SiteReport, 'days' | 'outages' | 'pages' | 'countries'>): string[] {
  const problems: string[] = []
  const pv = report.days.filter((d) => d.pageviews === 'differs').length
  const vis = report.days.filter((d) => d.visitors === 'differs').length
  const gaps = report.outages.filter((o) => o.zero !== 'both').length
  if (pv) problems.push(`pageviews differ by more than ${TOLERANCE.pageviews * 100}% on ${pv} of ${report.days.length} days`)
  if (vis) problems.push(`visitors differ by more than ${TOLERANCE.visitors * 100}% on ${vis} of ${report.days.length} days`)
  if (gaps) problems.push(`${gaps} day${gaps === 1 ? '' : 's'} with traffic on one side only`)
  if (report.pages.verdict === 'differs') problems.push('top pages differ')
  if (report.countries.verdict === 'differs') problems.push('top countries differ')
  return problems
}

export function verdictLine(reports: SiteReport[]): string {
  const bad = reports.filter((r) => r.problems.length)
  if (reports.length === 0) return 'Verdict: no sites to compare'
  if (bad.length === 0) return 'Verdict: within tolerance'
  return `Verdict: ${bad.length} of ${reports.length} sites off: ${bad.map((r) => `${r.name || r.site} (${r.problems.join('; ')})`).join(' | ')}`
}
