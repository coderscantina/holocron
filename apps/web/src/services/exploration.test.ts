import { describe, expect, it } from 'vitest'

import type { ExplorationState, Group } from '~/generated'

import {
  addCondition,
  applySegment,
  canonical,
  changeTone,
  changeWords,
  clearFilters,
  compareBlock,
  conditions,
  decodeFilter,
  decodeState,
  defaultState,
  encodeFilter,
  encodeState,
  exclude,
  fitRange,
  groupWithPrevious,
  include,
  overviewState,
  removeCondition,
  setRange,
  splitBy,
} from './exploration'

const site = '0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01'

/** The 0004 acceptance state: mobile signups with plan=pro, split by country and referrer. */
function acceptance(): ExplorationState {
  let state = defaultState(site, 'Europe/Vienna')
  state = { ...state, measures: ['events', 'visitor_days'], range: { kind: 'relative', unit: 'day', count: 30 } }
  state = addCondition(state, { field: 'device', cmp: 'eq', value: { t: 'string', v: 'mobile' } })
  state = addCondition(state, { field: 'event_name', cmp: 'eq', value: { t: 'string', v: 'signup' } })
  state = addCondition(state, { field: { scope: 'event', key: 'plan' }, cmp: 'eq', value: { t: 'string', v: 'pro' } })
  state = splitBy(state, 'country')
  return splitBy(state, 'referrer_domain')
}

describe('exploration state', () => {
  it('round-trips through the URL with repeated fields and nested groups', () => {
    let state = acceptance()
    state = addCondition(state, { field: 'device', cmp: 'eq', value: { t: 'string', v: 'tablet' } })
    state = groupWithPrevious(state, 3)
    const decoded = decodeState(encodeState(state))

    expect(decoded).toEqual({ ok: true, state })
    expect(state.filter?.items[2]).toMatchObject({ kind: 'group', op: 'or' })
    expect(conditions(state.filter).filter((c) => c.field === 'device')).toHaveLength(2)
  })

  it('writes keys in canonical order, so equal states encode equally', () => {
    expect(canonical({ b: 1, a: [{ d: null, c: 'x' }] })).toBe('{"a":[{"c":"x","d":null}],"b":1}')
    const state = acceptance()
    expect(canonical(Object.fromEntries(Object.entries(state).reverse()))).toBe(canonical(state))
  })

  it('refuses unreadable, foreign-version and oversize URLs instead of computing another report', () => {
    expect(decodeState('not base64 json')).toEqual({ ok: false, reason: 'unreadable' })
    const v2 = btoa(JSON.stringify({ ...acceptance(), v: 2 })).replace(/=+$/, '')
    expect(decodeState(v2)).toEqual({ ok: false, reason: 'version' })
    expect(decodeState('a'.repeat(9000))).toEqual({ ok: false, reason: 'oversize' })
  })

  it('reads an all-time range from a link', () => {
    const state: ExplorationState = { ...acceptance(), range: { kind: 'all' } }
    expect(decodeState(encodeState(state))).toEqual({ ok: true, state })
  })

  it('drills down by appending conditions with new IDs and never renumbers', () => {
    const state = acceptance()
    const drilled = include(state, 'country', { t: 'string', v: 'AT' })
    expect(conditions(drilled.filter).map((c) => c.id)).toEqual(['f1', 'f2', 'f3', 'f4'])
    expect(conditions(drilled.filter)[3]).toEqual({
      kind: 'cond',
      id: 'f4',
      field: 'country',
      cmp: 'eq',
      value: { t: 'string', v: 'AT' },
    })
    const removed = removeCondition(drilled, 'f2')
    expect(conditions(addCondition(removed, { field: 'path', cmp: 'is_set' }).filter).map((c) => c.id)).toEqual([
      'f1',
      'f3',
      'f4',
      'f5',
    ])
  })

  it('maps not-set and null cells to their operators', () => {
    const state = acceptance()
    expect(conditions(include(state, 'referrer_domain', { t: 'not_set' }).filter).at(-1)?.cmp).toBe('is_not_set')
    expect(conditions(exclude(state, 'referrer_domain', { t: 'not_set' })?.filter ?? null).at(-1)?.cmp).toBe('is_set')
    expect(exclude(state, { scope: 'event', key: 'coupon' }, { t: 'null' })).toBeNull()
  })

  it('clears filters without touching dates, measures, dimensions or visualization', () => {
    const state = { ...acceptance(), page: { size: 50, index: 3 } }
    const cleared = clearFilters(state)
    expect(cleared).toEqual({ ...state, filter: null, page: { size: 50, index: 0 } })
  })

  it('copies a segment with fresh IDs so editing the segment later changes nothing', () => {
    const segment: Group = {
      kind: 'group',
      op: 'or',
      items: [
        { kind: 'cond', id: 'f1', field: 'device', cmp: 'eq', value: { t: 'string', v: 'mobile' } },
        { kind: 'cond', id: 'f2', field: 'device', cmp: 'eq', value: { t: 'string', v: 'tablet' } },
      ],
    }
    const applied = applySegment(acceptance(), segment)
    expect(conditions(applied.filter).map((c) => c.id)).toEqual(['f1', 'f2', 'f3', 'f4', 'f5'])
    expect(applied.filter?.items.at(-1)).toMatchObject({ kind: 'group', op: 'or' })
  })
})

describe('filter URL parameter', () => {
  it('round-trips a filter tree in canonical form and refuses what is not one', () => {
    const filter = acceptance().filter
    if (!filter) throw new Error('the acceptance state has a filter')
    expect(encodeFilter({ items: filter.items, op: filter.op, kind: 'group' })).toBe(encodeFilter(filter))
    expect(decodeFilter(encodeFilter(filter))).toEqual({ ok: true, filter })

    const empty: Group = { kind: 'group', op: 'and', items: [] }
    expect(decodeFilter(encodeFilter(empty))).toEqual({ ok: false, reason: 'unreadable' })
    expect(decodeFilter(encodeState(acceptance()))).toEqual({ ok: false, reason: 'unreadable' })
    expect(decodeFilter('not base64 json')).toEqual({ ok: false, reason: 'unreadable' })
    expect(decodeFilter('a'.repeat(9000))).toEqual({ ok: false, reason: 'oversize' })
  })
})

describe('overview state', () => {
  it('opens on today by the hour against a typical weekday', () => {
    expect(overviewState(site, 'Europe/Vienna')).toMatchObject({
      range: { kind: 'relative', unit: 'day', count: 1 },
      granularity: 'hour',
      compare: 'typical_weekday',
      measures: ['pageviews', 'sessions', 'visitor_days', 'bounce_rate', 'span_mean'],
    })
  })

  it('keeps the state of an existing URL or saved view', () => {
    const saved = defaultState(site, 'Europe/Vienna')
    expect(saved).toMatchObject({ range: { kind: 'relative', unit: 'day', count: 7 }, compare: 'previous_period' })
    expect(decodeState(encodeState(saved))).toEqual({ ok: true, state: saved })
    const typical = overviewState(site, 'Europe/Vienna')
    expect(decodeState(encodeState(typical))).toEqual({ ok: true, state: typical })
  })
})

describe('comparison', () => {
  it('blocks a typical weekday beyond 31 days or for a measure without a baseline', () => {
    const state = overviewState(site, 'Europe/Vienna')
    expect(compareBlock(state, 'typical_weekday')).toBeNull()
    expect(compareBlock(setRange(state, { kind: 'relative', unit: 'day', count: 31 }), 'typical_weekday')).toBeNull()
    expect(compareBlock(setRange(state, { kind: 'relative', unit: 'day', count: 90 }), 'typical_weekday')).toEqual({
      reason: 'range',
      days: 31,
    })
    expect(compareBlock(setRange(state, { kind: 'days', start: '2026-09-01', end: '2026-10-02' }), 'typical_weekday')).toEqual({
      reason: 'range',
      days: 31,
    })
    expect(compareBlock({ ...state, measures: ['pageviews', 'revenue_net'] }, 'typical_weekday')).toEqual({
      reason: 'measure',
      measure: 'revenue_net',
    })
    expect(compareBlock(setRange(state, { kind: 'relative', unit: 'day', count: 90 }), 'previous_period')).toBeNull()
  })

  it('drops hourly buckets and the typical weekday when a new range cannot use them', () => {
    const state = overviewState(site, 'Europe/Vienna')
    expect(fitRange(state, { kind: 'relative', unit: 'day', count: 30 })).toMatchObject({ granularity: 'hour', compare: 'typical_weekday' })
    expect(fitRange(state, { kind: 'relative', unit: 'day', count: 90 })).toMatchObject({
      range: { kind: 'relative', unit: 'day', count: 90 },
      granularity: 'day',
      compare: 'previous_period',
    })
    expect(fitRange({ ...state, compare: 'none' }, { kind: 'relative', unit: 'day', count: 90 }).compare).toBe('none')
  })

  it('reads a change as above, below, about the same within 5 % or new', () => {
    const previous = { t: 'count', v: 100 } as const
    expect(changeWords({ previous, change: 12, relative_change: 0.12 })).toEqual({ kind: 'above', ratio: 0.12 })
    expect(changeWords({ previous, change: -8, relative_change: -0.08 })).toEqual({ kind: 'below', ratio: 0.08 })
    expect(changeWords({ previous, change: -5, relative_change: -0.05 })).toEqual({ kind: 'same' })
    expect(changeWords({ previous: { t: 'count', v: 0 }, change: 4, reason: 'zero_baseline' })).toEqual({ kind: 'new' })
    expect(changeWords({ previous: { t: 'count', v: 0 }, change: 0, reason: 'zero_baseline' })).toEqual({ kind: 'same' })
    expect(changeWords({ previous: { t: 'unavailable', reason: 'insufficient_history' }, reason: 'insufficient_history' })).toBeNull()
  })

  it('reads a rise as good news except for bounces, refunds and load times', () => {
    const previous = { t: 'count', v: 100 } as const
    const up = { previous, change: 12, relative_change: 0.12 }
    const down = { previous, change: -12, relative_change: -0.12 }
    expect(changeTone('pageviews', up)).toBe('good')
    expect(changeTone('pageviews', down)).toBe('bad')
    expect(changeTone('bounce_rate', up)).toBe('bad')
    expect(changeTone('p75', down)).toBe('good')
    expect(changeTone('sessions', { previous, change: 3, relative_change: 0.03 })).toBe('neutral')
    expect(changeTone({ stat: 'sum', field: { scope: 'event', key: 'price' } }, up)).toBe('neutral')
  })
})
