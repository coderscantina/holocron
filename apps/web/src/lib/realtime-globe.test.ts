import { describe, expect, it } from 'vitest'

import type { RecentEvent } from '~/generated'
import { age, arrivals, pinId } from '~/lib/realtime-globe'

describe('arrivals', () => {
  it('keeps only events after the newest one seen', () => {
    const event = (at: number): RecentEvent => ({ at, kind: 'pageview', path: '/' })
    expect(arrivals([event(30), event(20), event(10)], 20).map((e) => e.at)).toEqual([30])
  })
})

describe('pinId', () => {
  it('names the country and the 0.1° cell as a valid CSS identifier', () => {
    expect(pinId({ country: 'AT', lat: 48.2, lng: 16.4 })).toBe('AT482_164')
    expect(pinId({ country: 'AR', lat: -34.6, lng: -58.4 })).toBe('ARm346_m584')
  })
})

describe('age', () => {
  it('reads just now, then seconds, minutes and hours', () => {
    expect(age(9_999)).toEqual({ kind: 'now' })
    expect(age(-500)).toEqual({ kind: 'now' })
    expect(age(15_400)).toEqual({ kind: 'ago', value: 15, unit: 'second' })
    expect(age(119_000)).toEqual({ kind: 'ago', value: 1, unit: 'minute' })
    expect(age(2 * 3_600_000)).toEqual({ kind: 'ago', value: 2, unit: 'hour' })
  })
})
