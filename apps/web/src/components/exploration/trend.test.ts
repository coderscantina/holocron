import { afterEach, describe, expect, it } from 'vitest'

import type { Freshness, Projection, Series } from '~/generated'
import { composer, type LocaleCode } from '~/plugins/i18n'

import { bucketOf, bucketText, buckets, chartClock, consequences, nowMarker, type Phrase } from './trend'

/** Tuesday 13 October 2026 in Vienna (UTC+2) by the hour, measured up to 15:20 local time. */
const HOUR = 3_600_000
const midnight = Date.UTC(2026, 9, 12, 22)
const end = midnight + 24 * HOUR
const now = midnight + 15 * HOUR + 20 * 60_000
const measured = [44, 25, 13, 11, 10, 14, 33, 78, 131, 262, 301, 288, 245, 236, 251, 212]
const estimates = [300, 262, 296, 331, 365, 376, 342, 251, 137]
const typical = [40, 22, 14, 10, 9, 12, 30, 70, 120, 160, 190, 205, 210, 200, 195, 205, 230, 260, 290, 320, 330, 300, 220, 120]

const local = (hour: number) => `2026-10-13T${String(hour).padStart(2, '0')}:00`
const estimate = (value: number): Projection => ({ t: 'estimate', value, low: value - 16, high: value + 14, basis_days: 8 })

const today: Series = {
  granularity: 'hour',
  series_sum_differs: false,
  points: typical.map((baseline, hour) => ({
    start: midnight + hour * HOUR,
    local: local(hour),
    values: [{ t: 'count', v: measured[hour] ?? 0 }],
    compare: [{ previous: { t: 'count', v: baseline } }],
  })),
  projection: estimates.map((value, i) => ({ start: midnight + (15 + i) * HOUR, local: local(15 + i), values: [estimate(value)] })),
}
const tuesday = { kind: 'weekday', weekday: 2 } as const

const say = (phrase: Phrase | null, locale: LocaleCode = 'en') => {
  composer.locale.value = locale
  return phrase ? composer.t(phrase.key, phrase.named) : null
}

afterEach(() => {
  composer.locale.value = 'en'
})

describe('bucketOf', () => {
  it('finds the bucket holding an instant and nothing outside the range', () => {
    const points = [{ start: 0 }, { start: 10 }, { start: 20 }]
    expect(bucketOf(points, 15, 30)).toBe(1)
    expect(bucketOf(points, 30, 30)).toBe(-1)
    expect(bucketOf(points, -1, 30)).toBe(-1)
  })
})

describe('nowMarker', () => {
  it('sits at the clock inside the bucket that holds it', () => {
    const marker = nowMarker(today.points, { now, end })
    expect(marker).toMatchObject({ at: now, local: '2026-10-13T15:20', stale: false })
    expect(marker?.position).toBeCloseTo((15 + 1 / 3) / 24)
  })

  it('sits at the watermark while analytics is stale', () => {
    const watermark = midnight + 14 * HOUR + 45 * 60_000
    const marker = nowMarker(today.points, { now, end, watermark })
    expect(marker).toMatchObject({ at: watermark, local: '2026-10-13T14:45', stale: true })
    expect(marker?.position).toBeCloseTo(14.75 / 24)
  })

  it('reads a stale freshness as "Data until" the watermark and a current one as now', () => {
    const watermark = midnight + 14 * HOUR + 45 * 60_000
    const freshness = (stale: boolean): Freshness => ({
      watermark_batch: 7,
      watermark_sequence: 120,
      watermark_at: stale ? watermark : now,
      stale,
      corrected_through: 7,
      correction: 'corrected',
      dirty_days: 0,
      metric_version: 1,
      session_policy_versions: [1],
    })
    const resolved = { start: midnight, end, timezone: 'Europe/Vienna' }
    const marker = (stale: boolean) => nowMarker(today.points, chartClock(now, { resolved, freshness: freshness(stale) }))
    expect(marker(true)).toMatchObject({ at: watermark, stale: true })
    expect(marker(false)).toMatchObject({ at: now, stale: false })
  })

  it('is absent for a range that ended before now', () => {
    expect(nowMarker(today.points, { now: end + HOUR, end })).toBeNull()
  })
})

describe('bucketText', () => {
  const rows = buckets(today, 0)

  it('names a bucket still to come as an estimate with its band and basis, without a measured value', () => {
    const text = bucketText(rows[20]!, tuesday)
    expect(text.measured).toBe(false)
    expect(say(text.estimate)).toBe('Estimate 376, between 360 and 390')
    expect(say(text.basis)).toBe('The estimate comes from the last 8 Tuesdays.')
    expect(say(text.estimate, 'de')).toBe('Schätzung 376, zwischen 360 und 390')
    expect(say(text.basis, 'de')).toBe('Die Schätzung beruht auf den letzten 8 Dienstagen.')
  })

  it('keeps the measured value of the bucket holding now next to its estimate', () => {
    const text = bucketText(rows[15]!, tuesday)
    expect(text.measured).toBe(true)
    expect(say(text.estimate)).toBe('Estimate 300, between 284 and 314')
  })

  it('has no estimate for a measured bucket and falls back to baseline days without a weekday', () => {
    expect(bucketText(rows[9]!, tuesday)).toEqual({ measured: true, estimate: null, basis: null })
    expect(say(bucketText(rows[20]!, { kind: 'previous' }).basis)).toBe('The estimate comes from 8 baseline days.')
  })
})

describe('consequences', () => {
  it('names when the running total passes the next round number and the busiest hour ahead', () => {
    expect(consequences(buckets(today, 0), 'hour', { at: now, sumDiffers: false })).toEqual([
      { kind: 'passes', value: 3000, at: '2026-10-13T19:00' },
      { kind: 'busiest', start: '2026-10-13T20:00', end: '2026-10-13T21:00' },
    ])
  })

  it('leaves out the running total when the series does not add up to the total', () => {
    expect(consequences(buckets(today, 0), 'hour', { at: now, sumDiffers: true }).map((c) => c.kind)).toEqual(['busiest'])
  })

  it('says nothing without a projection', () => {
    expect(consequences(buckets({ ...today, projection: undefined }, 0), 'hour', { at: null, sumDiffers: false })).toEqual([])
  })
})
