import type { RecentEvent } from '~/generated'
import type { Coordinates } from '~/lib/globe'

/** The events newer than the newest one already seen, newest first, as the snapshot lists them. */
export function arrivals(recent: readonly RecentEvent[], seenUntil: number): RecentEvent[] {
  return recent.filter((event) => event.at > seenUntil)
}

/** A point on the globe with its country, as pins and events carry them. */
export type Place = Coordinates & { country: string }

/** Where an event happened; `null` without a country, which the server sends no location for. */
export function placeOf(event: RecentEvent): Place | null {
  const { country, lat, lng } = event
  return country !== undefined && lat !== undefined && lng !== undefined ? { country, lat, lng } : null
}

/** The newest arriving event and its place; `seq` tells two arrivals at one place apart. */
export type Spotlight = { seq: number; event: RecentEvent; place: Place }

/**
 * A pin's id, also its CSS anchor name (`--cobe-<id>`): the country and the 0.1° cell, so the
 * newest event finds the pin of its place.
 */
export function pinId({ country, lat, lng }: Place): string {
  const tenths = (degrees: number) => String(Math.round(degrees * 10)).replace('-', 'm')
  return `${country}${tenths(lat)}_${tenths(lng)}`
}

export type Age = { kind: 'now' } | { kind: 'ago'; value: number; unit: 'second' | 'minute' | 'hour' }

/** How old an event is, in the unit the feed shows: "just now" under 10 seconds. */
export function age(ms: number): Age {
  const seconds = Math.max(0, Math.floor(ms / 1000))
  if (seconds < 10) return { kind: 'now' }
  if (seconds < 60) return { kind: 'ago', value: seconds, unit: 'second' }
  if (seconds < 3600) return { kind: 'ago', value: Math.floor(seconds / 60), unit: 'minute' }
  return { kind: 'ago', value: Math.floor(seconds / 3600), unit: 'hour' }
}
