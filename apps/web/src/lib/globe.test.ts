import { describe, expect, it } from 'vitest'

import { densest, facing, nearestAngle } from '~/lib/globe'

const location = (lat: number, lng: number, weight: number) => ({ lat, lng, weight })

describe('facing', () => {
  it('turns a longitude to the front and tilts part of the way to its latitude', () => {
    expect(facing({ lat: 0, lng: 0 }).phi).toBeCloseTo(-Math.PI / 2)
    expect(facing({ lat: 0, lng: 90 }).phi).toBeCloseTo(-Math.PI)
    expect(facing({ lat: 50, lng: 0 }).theta).toBeCloseTo(50 * (Math.PI / 180) * 0.6)
    expect(facing({ lat: -89, lng: 0 }).theta).toBe(-0.7)
  })
})

describe('nearestAngle', () => {
  it('takes the short way round', () => {
    expect(nearestAngle(3, -3)).toBeCloseTo(-3 + 2 * Math.PI)
    expect(nearestAngle(0.5, 1)).toBeCloseTo(1)
  })
})

describe('densest', () => {
  it('picks the region with the most visitors, not the single biggest place', () => {
    const europe = [location(48.2, 16.4, 3), location(52.5, 13.4, 3), location(51.5, -0.1, 2)]
    const center = densest([location(40.7, -74, 5), ...europe])
    expect(center.lat).toBeGreaterThan(48)
    expect(center.lat).toBeLessThan(53)
    expect(center.lng).toBeGreaterThan(0)
    expect(center.lng).toBeLessThan(16.4)
  })

  it('rests on a fixed place without visitors', () => {
    expect(densest([])).toEqual({ lat: 20, lng: 10 })
  })
})
