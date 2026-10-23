import type { ExplorationState, GoalResult, Projection, ResolvedRange, SavedGoal } from '~/generated'
import { TYPICAL_WEEKDAY_RANGE_DAYS_MAX } from '~/generated/limits'
import { containsNow, rangeDays } from '~/services/exploration'
import { goalsState } from '~/services/reports'

import type { Estimate } from './headline'

/**
 * The overview's goal rows (design contract, Goals panel): one `goals` report over the
 * overview's range and filter against a typical weekday, and the pace each row states.
 * Nothing here formats or translates.
 */

/** Within this share of the typical value by now a goal reads as on pace. */
export const ON_PACE_BAND = 0.1

/**
 * The goals report for the overview's scope: the first eight active goals over the same range
 * and filter, against a typical weekday when the range allows one.
 */
export function goalPaceState(overview: ExplorationState, goals: SavedGoal[]): ExplorationState {
  const report = goalsState(overview.site, overview.timezone, goals)
  const fits = rangeDays(overview.range) <= TYPICAL_WEEKDAY_RANGE_DAYS_MAX
  return { ...report, range: overview.range, filter: overview.filter, compare: fits ? 'typical_weekday' : 'none' }
}

export type PaceState =
  | { kind: 'behind'; count: number }
  | { kind: 'even' }
  | { kind: 'ahead'; count: number }
  /** The baseline has fewer than the minimum days. */
  | { kind: 'no_history' }
  /** No comparison: the range is too long for a typical weekday. */
  | { kind: 'none' }

export interface GoalPaceRow {
  completions: number
  /** The typical value up to now, or over the whole range when it is past. */
  typical: number | null
  /** Where completions end, labelled as an estimate; only while the range contains now. */
  estimate: Estimate | null
  /** The range contains now, so the row speaks of pace instead of a finished total. */
  live: boolean
  state: PaceState
  /** Meter fill and the typical tick, 0 to 1 of the meter. */
  fill: number
  tick: number | null
}

function paceState(completions: number, typical: number): PaceState {
  const difference = completions - typical
  if (Math.abs(difference) <= typical * ON_PACE_BAND || difference === 0) return { kind: 'even' }
  return difference > 0 ? { kind: 'ahead', count: difference } : { kind: 'behind', count: -difference }
}

const estimateOf = (projection: Projection | undefined): Estimate | null =>
  projection?.t === 'estimate' ? projection : null

/**
 * One row per goal result. The meter spans the largest of completions, the typical value and
 * the estimate's upper band, so the fill and the tick share one scale.
 */
export function goalPaceRow(result: GoalResult, resolved: ResolvedRange, now: number): GoalPaceRow {
  const live = containsNow(resolved, now)
  const previous = result.compare?.completions.previous
  const typical = previous?.t === 'count' ? previous.v : null
  const estimate = live ? estimateOf(result.projection) : null
  const state: PaceState =
    typical !== null
      ? paceState(result.completions, typical)
      : result.compare
        ? { kind: 'no_history' }
        : { kind: 'none' }
  const scale = Math.max(1, result.completions, typical ?? 0, estimate?.high ?? 0)
  return {
    completions: result.completions,
    typical,
    estimate,
    live,
    state,
    fill: result.completions / scale,
    tick: typical === null ? null : typical / scale,
  }
}
