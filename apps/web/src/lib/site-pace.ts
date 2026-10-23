import type { Summary } from '~/generated'

/** Below this many pageviews, today and typical both, a percentage is noise: show the count. */
export const QUIET_VIEWS = 50

/** Within this share of the typical value, a site reads as in its usual range. */
const USUAL_RANGE = 0.05

/** What a fresh summary says about a site, in the order the site chip and menu check it. */
export type SitePace =
  | { kind: 'new' }
  | { kind: 'quiet'; views: number }
  | { kind: 'usual'; ratio: number }
  | { kind: 'above' | 'below'; ratio: number }

/**
 * The pace of a site's pageviews so far today against a typical weekday. Missing and stale
 * summaries have none: the site shows its name only.
 */
export function sitePace(summary: Summary | undefined): SitePace | null {
  if (!summary || summary.stale) return null
  if (summary.insufficient_history) return { kind: 'new' }
  const { today, typical, relative_change: ratio } = summary.pageviews
  return pageviewPace(today, typical, ratio)
}

/** The pace of pageviews so far against the typical value up to the same time. */
export function pageviewPace(today: number, typical: number | null, ratio: number | null): Exclude<SitePace, { kind: 'new' }> {
  if (ratio === null || Math.max(today, typical ?? 0) < QUIET_VIEWS) return { kind: 'quiet', views: today }
  if (Math.abs(ratio) < USUAL_RANGE) return { kind: 'usual', ratio }
  return { kind: ratio > 0 ? 'above' : 'below', ratio }
}

/** 0.124 → "+12", -0.081 → "−8" (a real minus sign), rounded to whole percent. */
export function signedPercent(ratio: number): string {
  const rounded = Math.round(ratio * 100)
  return `${rounded > 0 ? '+' : rounded < 0 ? '−' : '±'}${Math.abs(rounded)}`
}

/** The weekday "today" has in the summary's own timezone, as the locale spells it. */
export function summaryWeekday(summary: Summary, locale: string): string {
  return new Intl.DateTimeFormat(locale, { weekday: 'long', timeZone: summary.timezone }).format(summary.computed_at)
}

/** When a summary was computed, as a short local time. */
export function summaryTime(ms: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { timeStyle: 'short' }).format(ms)
}

/** Findings worth pointing at: none from a stale summary, which cannot say what is unusual today. */
export function findingCount(summary: Summary | undefined): number {
  return summary && !summary.stale ? summary.finding_count : 0
}
