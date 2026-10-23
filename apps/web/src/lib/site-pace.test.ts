import { describe, expect, it } from 'vitest'

import type { Summary } from '~/generated'
import { findingCount, signedPercent, sitePace, summaryWeekday } from '~/lib/site-pace'

const summary = (patch: Partial<Summary> = {}, pageviews: Partial<Summary['pageviews']> = {}): Summary => ({
  site: 'site',
  computed_at: Date.UTC(2026, 9, 13, 15, 15),
  watermark_batch: 1,
  watermark_sequence: 1,
  metric_version: 1,
  stale: false,
  timezone: 'Europe/Vienna',
  pageviews: { today: 520, typical: 464, relative_change: 0.12, projection: null, ...pageviews },
  visitor_days: { today: 0, typical: null, relative_change: null, projection: null },
  hourly_today: [],
  hourly_typical: [],
  insufficient_history: false,
  finding_count: 0,
  ...patch,
})

describe('sitePace', () => {
  it('has no pace for a missing or stale summary', () => {
    expect(sitePace(undefined)).toBeNull()
    expect(sitePace(summary({ stale: true }))).toBeNull()
  })

  it('calls a site with too little history new', () => {
    expect(sitePace(summary({ insufficient_history: true }, { typical: null, relative_change: null }))).toEqual({ kind: 'new' })
  })

  it('shows the count for almost no traffic', () => {
    expect(sitePace(summary({}, { today: 31, typical: 20, relative_change: 0.55 }))).toEqual({ kind: 'quiet', views: 31 })
    expect(sitePace(summary({}, { today: 0, typical: 0, relative_change: null }))).toEqual({ kind: 'quiet', views: 0 })
  })

  it('reads within five percent as the usual range', () => {
    expect(sitePace(summary({}, { relative_change: -0.049 }))).toEqual({ kind: 'usual', ratio: -0.049 })
  })

  it('reads above and below', () => {
    expect(sitePace(summary())).toEqual({ kind: 'above', ratio: 0.12 })
    expect(sitePace(summary({}, { relative_change: -0.08 }))).toEqual({ kind: 'below', ratio: -0.08 })
  })
})

describe('signedPercent', () => {
  it('signs the rounded percentage', () => {
    expect(signedPercent(0.124)).toBe('+12')
    expect(signedPercent(-0.081)).toBe('−8')
    expect(signedPercent(0.001)).toBe('±0')
  })
})

describe('summaryWeekday', () => {
  it('names the weekday in the summary timezone', () => {
    const late = summary({ computed_at: Date.UTC(2026, 9, 13, 22, 30) })
    expect(summaryWeekday(late, 'en')).toBe('Wednesday')
    expect(summaryWeekday(late, 'de')).toBe('Mittwoch')
  })
})

describe('findingCount', () => {
  it('counts the findings of a fresh summary only', () => {
    expect(findingCount(summary({ finding_count: 3 }))).toBe(3)
    expect(findingCount(summary({ finding_count: 3, stale: true }))).toBe(0)
    expect(findingCount(undefined)).toBe(0)
  })
})
