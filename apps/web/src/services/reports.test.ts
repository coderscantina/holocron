import { describe, expect, it } from 'vitest'

import type { SavedGoal } from '~/generated'

import { addCondition, conditions, decodeState, encodeState, nextId, removeCondition, trees } from './exploration'
import {
  addGoal,
  addStep,
  complete,
  funnelOf,
  funnelState,
  goalsOf,
  goalsState,
  moveStep,
  pickGroup,
  replaceGoal,
  sessionsFor,
  setSplit,
  upgradeFor,
} from './reports'

const site = '0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01'

const signup = (version: number, value = 'signup'): SavedGoal => ({
  id: 'goal-1',
  site,
  name: 'Signup',
  version,
  latest_version: version,
  definition: {
    predicate: { kind: 'group', op: 'and', items: [{ kind: 'cond', id: 'f1', field: 'event_name', cmp: 'eq', value: { t: 'string', v: value } }] },
    scope: 'session',
    counting: 'once',
  },
  archived: false,
  created_at: 0,
})

const ids = (state: ReturnType<typeof funnelState>) => trees(state).flatMap((tree) => conditions(tree).map((c) => c.id))

describe('report states', () => {
  it('keeps predicate IDs unique across the filter and every step', () => {
    let state = funnelState(site, 'Europe/Vienna')
    state = addCondition(state, { field: 'device', cmp: 'eq', value: { t: 'string', v: 'mobile' } })
    state = addStep(state, [{ field: 'event_name', cmp: 'eq', value: { t: 'string', v: 'signup' } }])
    state = addCondition(state, { field: 'country', cmp: 'eq', value: { t: 'string', v: 'AT' } }, { kind: 'step', index: 2 })

    const all = ids(state)
    expect(new Set(all).size).toBe(all.length)
    expect(nextId(state)).toBe(`f${all.length + 1}`)
    expect(funnelOf(state)?.steps[2]?.items).toHaveLength(2)
    expect(state.filter?.items).toHaveLength(1)
  })

  it('reorders steps and survives the URL with its report', () => {
    let state = addStep(funnelState(site, 'UTC'), [{ field: 'path', cmp: 'eq', value: { t: 'string', v: '/pricing' } }])
    const last = funnelOf(state)?.steps[2]
    state = moveStep(state, 2, -1)
    expect(funnelOf(state)?.steps[1]).toEqual(last)
    expect(decodeState(encodeState(state))).toEqual({ ok: true, state })
  })

  it('holds back a step without conditions instead of emptying the funnel', () => {
    const state = funnelState(site, 'UTC')
    const id = funnelOf(state)?.steps[1]?.items[0]
    const emptied = removeCondition(state, id?.kind === 'cond' ? id.id : '', { kind: 'step', index: 1 })
    expect(funnelOf(emptied)?.steps).toHaveLength(2)
    expect(complete(emptied)).toBe(false)
    expect(complete(state)).toBe(true)
  })

  it('stores goals inline with their version and offers newer versions as an upgrade', () => {
    const state = goalsState(site, 'UTC', [signup(1)])
    const goal = goalsOf(state)[0]
    expect(goal?.ref).toEqual({ id: 'goal-1', version: 1 })
    expect(goal && upgradeFor(goal, [signup(1)])).toBeNull()

    const newer = signup(2, 'signup_done')
    expect(goal && upgradeFor(goal, [newer])).toEqual(newer)
    const upgraded = replaceGoal(state, 0, newer)
    expect(goalsOf(upgraded)[0]?.ref).toEqual({ id: 'goal-1', version: 2 })
    // Copies get IDs that are fresh in the state, so two goals never share one.
    const twice = addGoal(upgraded, signup(1))
    const all = ids(twice)
    expect(new Set(all).size).toBe(all.length)
  })

  it('drills into sessions with the filter and the row predicate joined by AND', () => {
    let state = funnelState(site, 'Europe/Vienna')
    state = addCondition(state, { field: 'device', cmp: 'eq', value: { t: 'string', v: 'mobile' } })
    const step = funnelOf(state)?.steps[0]
    const sessions = sessionsFor(state, step ? [step] : [])
    expect(sessions.report?.kind).toBe('session_list')
    expect(sessions.range).toEqual(state.range)
    expect(conditions(sessions.filter).map((c) => c.field)).toEqual(['device', 'event_name'])
  })
})

describe('funnel split', () => {
  const plain = funnelState(site, 'Europe/Vienna')

  it('stays version 1 without a split and becomes version 2 with one', () => {
    expect(funnelOf(plain)?.v).toBe(1)
    expect(JSON.stringify(plain.report)).not.toContain('group_by')
    const split = setSplit(plain, 'country')
    expect(funnelOf(split)).toMatchObject({ v: 2, group_by: 'country' })
    expect(encodeState(setSplit(split, null))).toBe(encodeState(plain))
  })

  it('keeps the split through a URL round trip', () => {
    const split = setSplit(plain, { scope: 'event', key: 'plan' })
    const decoded = decodeState(encodeState(split))
    expect(decoded.ok && funnelOf(decoded.state)).toMatchObject({ v: 2, group_by: { scope: 'event', key: 'plan' } })
  })

  it('reads a picked group without the split, filtered to its value with a fresh ID', () => {
    const split = setSplit(plain, 'country')
    const picked = pickGroup(split, { t: 'string', v: 'AT' })
    expect(funnelOf(picked)).toMatchObject({ v: 1 })
    expect(funnelOf(picked)?.group_by).toBeUndefined()
    const added = conditions(picked.filter)
    expect(added).toMatchObject([{ field: 'country', cmp: 'eq', value: { t: 'string', v: 'AT' } }])
    expect(ids(picked)).toHaveLength(new Set(ids(picked)).size)
    expect(ids(split)).not.toContain(added[0]?.id)
  })

  it('filters the not-set group with is_not_set', () => {
    const picked = pickGroup(setSplit(plain, 'country'), { t: 'not_set' })
    expect(conditions(picked.filter)).toMatchObject([{ field: 'country', cmp: 'is_not_set' }])
  })
})
