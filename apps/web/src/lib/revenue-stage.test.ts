import { describe, expect, it } from 'vitest'

import type { Cell, Row } from '~/generated'
import { busiest, hourly, placesOf, topOf } from '~/lib/revenue-stage'

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

describe('placesOf', () => {
  it('puts countries at their centroids, largest first, discs by area', () => {
    const places = placesOf([row('EUR', { t: 'string', v: 'DE' }, 100), row('EUR', { t: 'string', v: 'AT' }, 400), row('EUR', { t: 'string', v: 'ZZ' }, 50)], 'EUR')
    expect(places.map((place) => place.country)).toEqual(['AT', 'DE'])
    expect(places[0]).toMatchObject({ lat: 47.5, lng: 14.6, scale: 1 })
    expect(places[1]!.scale).toBeCloseTo(0.5)
  })
})

describe('topOf', () => {
  it('picks the largest earner in the currency', () => {
    expect(topOf([row('EUR', { t: 'string', v: 'Direct' }, 100), row('EUR', { t: 'string', v: 'Search' }, 300)], 'EUR')?.key).toEqual([
      { t: 'string', v: 'Search' },
    ])
  })
})
