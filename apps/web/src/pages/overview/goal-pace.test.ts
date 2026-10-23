import { describe, expect, it } from 'vitest'

import type { GoalResult, ResolvedRange, SavedGoal } from '~/generated'
import { overviewState } from '~/services/exploration'

import { goalPaceRow, goalPaceState } from './goal-pace'

/** Tuesday 13 October 2026 in Vienna, at 15:20 local time. */
const today: ResolvedRange = { start: Date.UTC(2026, 9, 12, 22), end: Date.UTC(2026, 9, 13, 22), timezone: 'Europe/Vienna' }
const now = Date.UTC(2026, 9, 13, 13, 20)
const tomorrow = today.end + 3_600_000

const result = (completions: number, typical: number | null, projection?: GoalResult['projection']): GoalResult => ({
  completions,
  denominator: 1000,
  rate: { t: 'ratio', v: completions / 1000, numerator: completions, denominator: 1000 },
  coverage: { with_identity: 1000, eligible_events: 1000 },
  compare: {
    completions: {
      previous: typical === null ? { t: 'unavailable', reason: 'insufficient_history' } : { t: 'count', v: typical },
    },
    rate: { previous: { t: 'unavailable', reason: 'insufficient_history' } },
  },
  projection,
})

const saved = (n: number): SavedGoal => ({
  id: `goal-${n}`,
  site: 'site',
  name: `Goal ${n}`,
  version: 1,
  latest_version: 1,
  definition: { predicate: { kind: 'group', op: 'and', items: [] }, scope: 'session', counting: 'once' },
  archived: false,
  created_at: 0,
})

describe('goal pace', () => {
  it('asks for up to eight goals over the overview range and filter against a typical weekday', () => {
    const overview = { ...overviewState('site', 'Europe/Vienna'), range: { kind: 'relative', unit: 'day', count: 7 } } as const
    const state = goalPaceState(overview, Array.from({ length: 10 }, (_, i) => saved(i)))
    expect(state.report?.kind === 'goals' && state.report.goals.length).toBe(8)
    expect([state.range, state.compare]).toEqual([overview.range, 'typical_weekday'])
    expect(goalPaceState({ ...overview, range: { kind: 'relative', unit: 'day', count: 90 } }, [saved(1)]).compare).toBe('none')
  })

  it('reads behind, on pace within ten percent, and ahead of the typical value by now', () => {
    expect(goalPaceRow(result(41, 46), today, now).state).toEqual({ kind: 'behind', count: 5 })
    expect(goalPaceRow(result(50, 46), today, now).state).toEqual({ kind: 'even' })
    expect(goalPaceRow(result(0, 0), today, now).state).toEqual({ kind: 'even' })
    expect(goalPaceRow(result(23, 18), today, now).state).toEqual({ kind: 'ahead', count: 5 })
  })

  it('keeps the estimate, with its band, only while the range contains now', () => {
    const estimate = { t: 'estimate', value: 36, low: 33, high: 40, basis_days: 8 } as const
    const live = goalPaceRow(result(23, 18, estimate), today, now)
    expect([live.live, live.estimate, live.fill, live.tick]).toEqual([true, estimate, 23 / 40, 18 / 40])
    const past = goalPaceRow(result(23, 18, estimate), today, tomorrow)
    expect([past.live, past.estimate, past.tick]).toEqual([false, null, 18 / 23])
  })

  it('says when there is not enough history, and when there is no comparison', () => {
    const insufficient = goalPaceRow(result(12, null, { t: 'unavailable', reason: 'insufficient_history' }), today, now)
    expect([insufficient.state, insufficient.typical, insufficient.estimate, insufficient.tick]).toEqual([
      { kind: 'no_history' },
      null,
      null,
      null,
    ])
    expect(goalPaceRow({ ...result(12, 10), compare: undefined }, today, now).state).toEqual({ kind: 'none' })
  })
})
