import type { Metric } from '~/generated'

/**
 * How long a step took, as the two largest units that are not zero: 159000 ms is 2 min 39 s,
 * 273600000 ms is 3 d 4 h. Below a second there is no unit to count in.
 */

export type DurationUnit = 'd' | 'h' | 'min' | 's'

const UNITS: readonly (readonly [DurationUnit, number])[] = [
  ['d', 86_400_000],
  ['h', 3_600_000],
  ['min', 60_000],
  ['s', 1000],
]

/** The parts of a duration, largest first; empty below one second. */
export function durationParts(millis: number): { unit: DurationUnit; value: number }[] {
  const start = UNITS.findIndex(([, size]) => millis >= size)
  if (start < 0) return []
  const parts: { unit: DurationUnit; value: number }[] = []
  let rest = millis
  for (const [unit, size] of UNITS.slice(start, start + 2)) {
    const value = Math.floor(rest / size)
    rest -= value * size
    if (value > 0) parts.push({ unit, value })
  }
  return parts
}

/** The milliseconds of a metric that has them; `null` for a step nobody reached. */
export const millisOf = (metric: Metric): number | null => (metric.t === 'millis' ? metric.v : null)

/** Bucket labels of a duration's counts, in the order the server returns them. */
export const DURATION_BUCKETS = ['under1h', 'h1to24', 'd1to7', 'd7to30', 'd30'] as const
