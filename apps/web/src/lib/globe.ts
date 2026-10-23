/** Orientation math of the dotted globe (components/globe): where it faces and where it rests. */

/** Where the globe faces, in cobe's terms: `phi` turns around the poles, `theta` tilts. */
export type Orientation = { phi: number; theta: number }

export type Coordinates = { lat: number; lng: number }

const RADIANS = Math.PI / 180
/** How far the globe tilts toward a place's latitude, so northern places sit above the middle. */
const TILT = 0.6
const MAX_THETA = 0.7
/** Places within this angle count as one region when finding where most visitors are. */
const REGION_RADIANS = 25 * RADIANS
/** Where the globe rests without places: Europe and Africa in the middle. */
const DEFAULT_REST: Coordinates = { lat: 20, lng: 10 }

/**
 * The orientation that brings a place to the middle of the globe. cobe puts longitude `lng`
 * in front at `phi = 3π/2 − lng`; the tilt goes part of the way to the latitude.
 */
export function facing({ lat, lng }: Coordinates): Orientation {
  const theta = Math.max(-MAX_THETA, Math.min(MAX_THETA, lat * RADIANS * TILT))
  return { phi: normalizeAngle((3 * Math.PI) / 2 - lng * RADIANS), theta }
}

/** An angle in `[−π, π)`. */
export function normalizeAngle(angle: number): number {
  const turn = 2 * Math.PI
  return ((((angle + Math.PI) % turn) + turn) % turn) - Math.PI
}

/** The target angle closest to `from`, so a turn takes the short way round. */
export function nearestAngle(from: number, to: number): number {
  return from + normalizeAngle(to - from)
}

const toVector = ({ lat, lng }: Coordinates): [number, number, number] => {
  const phi = lat * RADIANS
  const lambda = lng * RADIANS
  return [Math.cos(phi) * Math.cos(lambda), Math.cos(phi) * Math.sin(lambda), Math.sin(phi)]
}

const angleBetween = (a: [number, number, number], b: [number, number, number]) =>
  Math.acos(Math.max(-1, Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2])))

/** A place with what it holds: visitors, revenue. */
export type Weighted = Coordinates & { weight: number }

/**
 * Where most of the weight is: the region (places within 25°) holding the most, as the
 * weighted middle of its places. Without places, a fixed resting place.
 */
export function densest(places: readonly Weighted[]): Coordinates {
  const points = places.map((place) => ({ vector: toVector(place), visitors: place.weight }))
  let best: { weight: number; sum: [number, number, number] } | null = null
  for (const point of points) {
    const sum: [number, number, number] = [0, 0, 0]
    let weight = 0
    for (const other of points) {
      if (angleBetween(point.vector, other.vector) > REGION_RADIANS) continue
      weight += other.visitors
      for (const axis of [0, 1, 2] as const) sum[axis] += other.vector[axis] * other.visitors
    }
    if (!best || weight > best.weight) best = { weight, sum }
  }
  if (!best) return DEFAULT_REST
  const [x, y, z] = best.sum
  return { lat: Math.atan2(z, Math.hypot(x, y)) / RADIANS, lng: Math.atan2(y, x) / RADIANS }
}
