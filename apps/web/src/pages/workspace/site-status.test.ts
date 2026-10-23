import { describe, expect, it } from 'vitest'

import type { Summary } from '~/generated'

import { activeThisHour, arrangeSites, hourOfDay, needingAttention, sortSites, sparkline } from './site-status'

const summary = (site: string, patch: Partial<Summary> = {}, change: number | null = 0): Summary => ({
  site,
  computed_at: Date.UTC(2026, 9, 13, 15, 15),
  watermark_batch: 1,
  watermark_sequence: 1,
  metric_version: 1,
  stale: false,
  timezone: 'Europe/Vienna',
  pageviews: { today: 100, typical: 100, relative_change: change, projection: null },
  visitor_days: { today: 0, typical: null, relative_change: null, projection: null },
  hourly_today: [],
  hourly_typical: [],
  insufficient_history: false,
  finding_count: 0,
  ...patch,
})

const site = (id: string, domain = `${id}.example`) => ({ id, name: id, domain })
const mapOf = (...list: Summary[]) => new Map(list.map((entry) => [entry.site, entry]))

describe('sortSites', () => {
  it('ranks findings, then absolute pace, then name; sites without a fresh summary come last', () => {
    const summaries = mapOf(
      summary('calm'),
      summary('drop', {}, -0.4),
      summary('rise', {}, 0.2),
      summary('alerted', { finding_count: 1 }),
      summary('twin', {}, 0.2),
      summary('stale', { stale: true, finding_count: 5 })
    )
    const sites = ['calm', 'drop', 'missing', 'rise', 'stale', 'alerted', 'twin'].map((id) => site(id))
    expect(sortSites(sites, summaries).map((entry) => entry.id)).toEqual(['alerted', 'drop', 'rise', 'twin', 'calm', 'missing', 'stale'])
  })
})

describe('arrangeSites', () => {
  const many = (count: number) => Array.from({ length: count }, (_, index) => site(`s${index}`, index % 2 ? 'a.example' : 'b.example'))

  it('stays one list without a heading at 8 sites', () => {
    const groups = arrangeSites(many(8), new Map())
    expect(groups).toHaveLength(1)
    expect(groups[0]?.baseDomain).toBe('')
  })

  it('groups by base domain above 8 sites', () => {
    expect(arrangeSites(many(9), new Map()).map((group) => group.baseDomain)).toEqual(['a.example', 'b.example'])
  })
})

describe('needingAttention', () => {
  it('counts sites with a fresh summary and findings', () => {
    const summaries = mapOf(summary('a', { finding_count: 2 }), summary('b'), summary('c', { stale: true, finding_count: 1 }))
    expect(needingAttention(['a', 'b', 'c', 'd'].map((id) => site(id)), summaries)).toBe(1)
  })
})

describe('sparkline', () => {
  it('draws today up to the last hour with traffic and breaks typical at missing hours', () => {
    const geometry = sparkline(
      summary('a', { hourly_today: [1, 2, 3, 0, 0, 0], hourly_typical: [1, 1, null, 2, 2, 2] })
    )
    expect(geometry.today.split(' ')).toHaveLength(3)
    expect(geometry.typical.map((run) => run.split(' ').length)).toEqual([2, 3])
  })

  it('has no typical line without history', () => {
    expect(sparkline(summary('a', { hourly_today: [1, 2], hourly_typical: [null, null] })).typical).toEqual([])
  })
})

describe('hourOfDay', () => {
  it('counts hours since local midnight, across a daylight saving change', () => {
    expect(hourOfDay(Date.UTC(2026, 9, 13, 15, 15), 'Europe/Vienna')).toBe(17)
    expect(hourOfDay(Date.UTC(2026, 9, 25, 9, 0), 'Europe/Vienna')).toBe(11)
    expect(hourOfDay(Date.UTC(2026, 2, 29, 9, 0), 'Europe/Vienna')).toBe(10)
  })
})

describe('activeThisHour', () => {
  const hours = (current: number) => Array.from({ length: 24 }, (_, hour) => (hour === 17 ? current : hour < 17 ? 4 : 0))

  it('reads the hour the summary was computed in', () => {
    expect(activeThisHour(summary('a', { hourly_today: hours(3) }))).toBe(true)
    expect(activeThisHour(summary('a', { hourly_today: hours(0) }))).toBe(false)
    expect(activeThisHour(summary('a', { hourly_today: hours(3), stale: true }))).toBe(false)
  })
})
