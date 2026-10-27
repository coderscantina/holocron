import { describe, expect, it } from 'vitest'

import type { Cell, Row, Series } from '~/generated'
import { bestPayer, busiest, hourly, pace, traffic } from '~/lib/revenue-summary'

const money = (currency: string, minor: number) => ({
  t: 'money' as const,
  v: [{ currency, exponent: 2, minor_units: minor, amount: (minor / 100).toFixed(2) }],
})
const row = (currency: string, key: Cell, minor: number): Row => ({ key: [{ t: 'string', v: currency }, key], values: [money(currency, minor)] })

describe('hourly', () => {
  it('keeps one currency and fills the hours without revenue with 0', () => {
    const hours = hourly([row('EUR', { t: 'integer', v: 20 }, 500), row('USD', { t: 'integer', v: 20 }, 900)], 'EUR')
    expect(hours).toHaveLength(24)
    expect(hours[20]).toBe(500)
    expect(hours[3]).toBe(0)
  })
})

describe('busiest', () => {
  it('finds the busiest stretch, wrapping past midnight', () => {
    const hours = Array.from({ length: 24 }, () => 0)
    hours[23] = 40
    hours[0] = 40
    hours[12] = 20
    expect(busiest(hours, 2)).toEqual({ start: 23, hours: 2, share: 0.8 })
    expect(busiest(Array.from({ length: 24 }, () => 0))).toBeNull()
  })
})

const count = (v: number) => ({ t: 'count' as const, v })
const day = (index: number, minor: number, previous?: number) => ({
  start: index,
  local: `2026-10-0${index + 1}T00:00`,
  values: [money('EUR', minor)],
  compare: previous === undefined ? undefined : [{ previous: money('EUR', previous) }],
})

describe('pace', () => {
  const series = (points: Series['points'], projection?: Series['projection']): Series => ({
    granularity: 'day',
    points,
    series_sum_differs: false,
    projection,
  })

  it('runs the totals up to now and the comparison over the whole range', () => {
    const result = pace(series([day(0, 100, 50), day(1, 200, 50), day(2, 0, 50)]), 0, 'EUR', 1)
    expect(result.points.map((point) => point.actual)).toEqual([100, 300, null])
    expect(result.points.map((point) => point.previous)).toEqual([50, 100, 150])
    expect(result).toMatchObject({ actual: 300, previous: 100, estimate: null })
  })

  it('shapes the estimate by the projected buckets and ends at the projected total', () => {
    const band = (value: number) => ({ t: 'money_estimate' as const, v: [{ currency: 'EUR', exponent: 2, value, low: value, high: value }], basis_days: 8 })
    const projected = series(
      [day(0, 100), day(1, 40), day(2, 0), day(3, 0)],
      [1, 2, 3].map((start) => ({ start, local: '', values: [band([0, 100, 100, 200][start] ?? 0)] }))
    )
    const result = pace(series([day(0, 100), day(1, 40), day(2, 0), day(3, 0)]), 0, 'EUR', 1, {
      series: projected,
      total: { ...band(500), v: [{ currency: 'EUR', exponent: 2, value: 500, low: 400, high: 600 }] },
    })
    // Remaining: 60 of bucket 1, then 100 and 200; the 360 still to come spread in that shape.
    expect(result.points[1]?.estimate).toBeNull()
    expect(result.points[2]?.estimate?.value).toBeCloseTo(140 + 360 * (160 / 360))
    expect(result.points[3]?.estimate).toEqual({ value: 500, low: 400, high: 600 })
  })
})

describe('traffic', () => {
  const paying = (source: string, net: number, visitors: number): Row => ({
    key: [{ t: 'string', v: 'EUR' }, { t: 'string', v: source }],
    values: [money('EUR', net), count(1), count(visitors)],
  })
  const all = (source: string, visitors: number): Row => ({ key: [{ t: 'string', v: source }], values: [count(visitors)] })

  it('ranks by revenue, keeps the rest as one row and compares revenue with traffic shares', () => {
    const sources = traffic(
      [paying('Email', 300, 6), paying('Search', 600, 10), paying('Social', 50, 1), paying('Referral', 50, 1)],
      [all('Email', 60), all('Search', 600), all('Social', 300), all('Referral', 40)],
      'EUR',
      { net: 1000, visitors: 1000 },
      2
    )
    expect(sources.map((source) => source.key)).toEqual([{ t: 'string', v: 'Search' }, { t: 'string', v: 'Email' }, null])
    expect(sources[0]).toMatchObject({ revenueShare: 0.6, visitorShare: 0.6, pays: 'even', perVisitor: 1 })
    expect(sources[1]).toMatchObject({ pays: 'more', perVisitor: 5, conversion: 0.1 })
    expect(sources[2]).toMatchObject({ net: 100, visitors: 340, paying: 2, pays: 'less' })
  })

  it('names the best payer only when it clearly beats the site rate', () => {
    const sources = traffic([paying('Email', 300, 6), paying('Search', 700, 10)], [all('Email', 60), all('Search', 940)], 'EUR', { net: 1000, visitors: 1000 })
    expect(bestPayer(sources, 1)?.key).toEqual({ t: 'string', v: 'Email' })
    expect(bestPayer(sources, 4)).toBeNull()
  })
})
