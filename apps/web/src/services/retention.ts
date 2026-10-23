import {
  REPORT_RETENTION_DAY_INTERVALS_MAX,
  REPORT_RETENTION_WEEK_INTERVALS_MAX,
  type ExplorationState,
  type RetentionReport,
} from '~/generated'

import { defaultState } from './exploration'

export type CohortPeriod = 'day' | 'week'

/** Offsets the interval menu offers per cohort period. */
export const intervalChoices: Record<CohortPeriod, number[]> = {
  day: [7, 14, 30],
  week: [4, 8, 12, 26],
}

const maxIntervals: Record<CohortPeriod, number> = {
  day: REPORT_RETENTION_DAY_INTERVALS_MAX,
  week: REPORT_RETENTION_WEEK_INTERVALS_MAX,
}

const report = (intervals: number): RetentionReport => ({ v: 1, intervals })

/** Weekly cohorts of the last eight weeks, eight offsets each. */
export function retentionState(site: string, timezone: string): ExplorationState {
  return {
    ...defaultState(site, timezone),
    measures: [],
    range: { kind: 'relative', unit: 'week', count: 8 },
    granularity: 'week',
    compare: 'none',
    visualization: 'cohort',
    report: { kind: 'retention', ...report(8) },
  }
}

export const cohortPeriod = (state: ExplorationState): CohortPeriod => (state.granularity === 'day' ? 'day' : 'week')

export const intervalsOf = (state: ExplorationState): number =>
  state.report?.kind === 'retention' ? state.report.intervals : 0

/** Switches between daily and weekly cohorts with a range and offsets that suit the period. */
export function setCohortPeriod(state: ExplorationState, period: CohortPeriod): ExplorationState {
  if (cohortPeriod(state) === period) return state
  const intervals = period === 'day' ? 14 : 8
  return {
    ...state,
    granularity: period,
    range: { kind: 'relative', unit: period, count: intervals },
    report: { kind: 'retention', ...report(intervals) },
  }
}

/** Sets the number of offsets, clamped to the period's limit (0004 Limits). */
export function setIntervals(state: ExplorationState, intervals: number): ExplorationState {
  const clamped = Math.min(Math.max(1, Math.round(intervals)), maxIntervals[cohortPeriod(state)])
  return { ...state, report: { kind: 'retention', ...report(clamped) } }
}
