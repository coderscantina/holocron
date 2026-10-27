/** Shapes of the event board: each event's counts over the range as a row of shaded cells. */

/** The faintest cell, so an empty bucket still shows as a slot of the row. */
export const SHADE_FLOOR = 0.08

/**
 * Cell opacities of one event's counts, scaled from zero to the event's own busiest bucket.
 * Squared, so ordinary days recede and the buckets that stand out read at a glance.
 */
export function shades(values: readonly number[]): number[] {
  const max = Math.max(0, ...values)
  return values.map((value) => (max > 0 ? SHADE_FLOOR + (1 - SHADE_FLOOR) * (Math.max(0, value) / max) ** 2 : SHADE_FLOOR))
}
