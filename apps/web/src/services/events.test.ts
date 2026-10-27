import { describe, expect, it } from 'vitest'

import type { ExplorationState, Group } from '~/generated'

import {
  analyse,
  boardState,
  eventSeriesState,
  eventsState,
  pivotOf,
  propertyOf,
  selectedEvent,
  selectEvent,
  setPivot,
  showEvents,
  showProperties,
  statsPanel,
  tabOf,
  trafficState,
} from './events'
import { conditions, decodeState, encodeState } from './exploration'

const site = '0d2c9a6e-5a3b-4f1e-9b1c-7e2f1a4d8c01'
const plan = { scope: 'event', key: 'plan' } as const
const qty = { scope: 'event', key: 'qty' } as const

describe('events states', () => {
  it('list custom events by name and survive the URL', () => {
    const state = eventsState(site, 'Europe/Vienna')
    expect(tabOf(state)).toBe('events')
    expect(conditions(state.filter)).toEqual([{ kind: 'cond', id: 'f1', field: 'event_name', cmp: 'is_set' }])
    const decoded = decodeState(encodeState(state))
    expect(decoded.ok && decoded.state).toEqual(state)
  })

  it('select one event with one condition, keep other conditions, and narrow an OR root', () => {
    const opened = showProperties(eventsState(site, 'UTC'), 'signup')
    expect(tabOf(opened)).toBe('properties')
    expect(selectedEvent(opened)).toBe('signup')

    const other = selectEvent(opened, 'purchase')
    expect(selectedEvent(other)).toBe('purchase')
    expect(conditions(other.filter).map((c) => c.id)).toEqual(['f1', 'f2'])
    expect(conditions(other.filter).filter((c) => c.field === 'event_name' && c.cmp === 'eq')).toHaveLength(1)
    expect(selectedEvent(showEvents(other))).toBeNull()

    const or: Group = { kind: 'group', op: 'or', items: conditions(eventsState(site, 'UTC').filter) }
    const narrowed = selectEvent({ ...opened, filter: or }, 'signup')
    expect(narrowed.filter?.op).toBe('and')
    expect(narrowed.filter?.items[0]).toEqual(or)
  })

  it('analyse numbers with sum, mean and statistics, other types by count', () => {
    const base = showProperties(eventsState(site, 'UTC'), 'signup')
    const numbers = analyse(base, qty, ['integer', 'string'])
    expect(numbers.measures).toEqual(['events', { stat: 'sum', field: qty }, { stat: 'mean', field: qty }])
    expect(numbers.visualization).toBe('bar')
    expect(statsPanel(numbers, ['integer'])[0]?.measures).toHaveLength(5)

    const text = analyse(base, plan, ['string'])
    expect(text.measures).toEqual(['events', 'visitor_days'])
    expect(statsPanel(text, ['string'])).toEqual([])
    expect(analyse(base, { scope: 'session', key: 'tier' }, ['integer']).measures[0]).toBe('sessions')
    expect(analyse(base, { scope: 'event', key: 'due' }, ['date']).sort).toEqual([
      { by: { scope: 'event', key: 'due' }, dir: 'asc' },
    ])
  })

  it('pivot by a second property and drop sort keys that leave the view', () => {
    const values: ExplorationState = { ...analyse(showProperties(eventsState(site, 'UTC')), plan, ['string']), sort: [{ by: qty, dir: 'desc' }] }
    const pivoted = setPivot(values, qty)
    expect(propertyOf(pivoted)).toEqual(plan)
    expect(pivotOf(pivoted)).toEqual(qty)
    expect(setPivot(pivoted, null).sort).toEqual([])
    expect(setPivot(pivoted, plan)).toBe(pivoted)
  })
})

describe('event board states', () => {
  const filtered = selectEvent(eventsState(site, 'UTC'), 'signup')

  it('lists the top events without an event selection', () => {
    const board = boardState(filtered)
    expect(board).toMatchObject({ dimensions: ['event_name'], measures: ['events', 'visitor_days', 'revenue_net'], page: { size: 12 } })
    expect(selectedEvent(board)).toBeNull()
  })

  it('reads all traffic for reach and one event for a row', () => {
    expect(conditions(trafficState(eventsState(site, 'UTC')).filter)).toEqual([])
    expect(selectedEvent(eventSeriesState(filtered, 'purchase'))).toBe('purchase')
  })
})
