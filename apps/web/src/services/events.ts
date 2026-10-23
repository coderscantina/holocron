import type { Condition, ExplorationState, Field, Group, Measure, Panel, PropertyField, PropertyType } from '~/generated'

import { addCondition, canonical, defaultState, nextId, normalize } from './exploration'

/**
 * The events screen as plain exploration states (CAP-TRAFFIC-06 and 07). The list is split by
 * `event_name`; the properties view selects an event with one `event_name eq` condition and a
 * property with the first dimension. Everything stays in the one URL state, so saving,
 * pinning, export and Back work as in Explore.
 */

/** Custom events only: pageviews carry no event name. */
const customEvents: Omit<Condition, 'id' | 'kind'> = { field: 'event_name', cmp: 'is_set' }

/** Event names with counts, visitor-days and sessions over the last 30 days, compared. */
export function eventsState(site: string, timezone: string): ExplorationState {
  return addCondition(
    {
      ...defaultState(site, timezone),
      range: { kind: 'relative', unit: 'day', count: 30 },
      measures: ['events', 'visitor_days', 'sessions'],
      dimensions: ['event_name'],
      sort: [{ by: 'events', dir: 'desc' }],
    },
    customEvents
  )
}

export type EventsTab = 'events' | 'properties'

export const tabOf = (state: ExplorationState): EventsTab =>
  state.dimensions[0] === 'event_name' ? 'events' : 'properties'

const isProperty = (field: Field | undefined): field is PropertyField => typeof field === 'object'

/** The analysed property: the first dimension of the properties view. */
export const propertyOf = (state: ExplorationState): PropertyField | null => {
  const field = state.dimensions[0]
  return isProperty(field) ? field : null
}

/** The second dimension of the properties view, the property the values are pivoted by. */
export const pivotOf = (state: ExplorationState): Field | null => (propertyOf(state) ? (state.dimensions[1] ?? null) : null)

/** The root conditions that select one event. Nested groups are the person's own filter. */
const selections = (state: ExplorationState): Condition[] =>
  state.filter?.op === 'and'
    ? state.filter.items.flatMap((item) =>
        item.kind === 'cond' && item.field === 'event_name' && item.cmp === 'eq' && item.value?.t === 'string' ? [item] : []
      )
    : []

/** The selected event name, or `null` for all custom events. */
export const selectedEvent = (state: ExplorationState): string | null => {
  const [only] = selections(state)
  return only?.value?.t === 'string' ? only.value.v : null
}

/**
 * Replaces the event selection; `null` selects all custom events again. An OR root becomes one
 * item of a new AND root, so the selection narrows the filter instead of widening it.
 */
export function selectEvent(state: ExplorationState, name: string | null): ExplorationState {
  const dropped = new Set(selections(state).map((c) => c.id))
  const items = state.filter?.items.filter((item) => item.kind !== 'cond' || !dropped.has(item.id)) ?? []
  const base = normalize({ ...state, filter: state.filter ? { ...state.filter, items } : null })
  const page = { ...base.page, index: 0 }
  if (name === null) return { ...base, page }
  const condition: Condition = {
    kind: 'cond',
    id: nextId(base),
    field: 'event_name',
    cmp: 'eq',
    value: { t: 'string', v: name },
  }
  const root = base.filter
  const filter: Group =
    root?.op === 'or'
      ? { kind: 'group', op: 'and', items: [root, condition] }
      : { kind: 'group', op: 'and', items: [...(root?.items ?? []), condition] }
  return { ...base, filter, page }
}

/** The list of event names, without an event selection. */
export function showEvents(state: ExplorationState): ExplorationState {
  const next = selectEvent(state, null)
  return {
    ...next,
    measures: ['events', 'visitor_days', 'sessions'],
    dimensions: ['event_name'],
    sort: [{ by: 'events', dir: 'desc' }],
    visualization: 'bar',
  }
}

/** The properties view of one event, or of all custom events, before a property is chosen. */
export function showProperties(state: ExplorationState, event: string | null = selectedEvent(state)): ExplorationState {
  const next = selectEvent(state, event)
  return { ...next, measures: ['events', 'visitor_days'], dimensions: [], sort: [], visualization: 'bar' }
}

/** Integer and decimal rows have statistics; other types of the same key are not rows of them (0003). */
export const isNumeric = (types: PropertyType[]) => types.includes('integer') || types.includes('decimal')

const stat = (kind: 'count' | 'sum' | 'mean' | 'min' | 'max', field: PropertyField): Measure => ({ stat: kind, field })

/**
 * Values of one property with counts. Numeric event properties add the sum and mean, per value
 * and over time. Session properties count sessions, since the server has no statistics over them.
 */
export function analyse(state: ExplorationState, field: PropertyField, types: PropertyType[]): ExplorationState {
  const numeric = field.scope === 'event' && isNumeric(types)
  const count: Measure = field.scope === 'event' ? 'events' : 'sessions'
  const measures: Measure[] = numeric ? [count, stat('sum', field), stat('mean', field)] : [count, 'visitor_days']
  // Dates read best in date order; everything else by how often it occurs.
  const dates = types.length > 0 && types.every((type) => type === 'date' || type === 'null')
  return {
    ...state,
    measures,
    dimensions: [field],
    sort: [dates ? { by: field, dir: 'asc' } : { by: count, dir: 'desc' }],
    visualization: numeric ? 'bar' : 'table',
    page: { ...state.page, index: 0 },
  }
}

/** Pivots the values of the analysed property by a second field, or drops the pivot. */
export function setPivot(state: ExplorationState, pivot: Field | null): ExplorationState {
  const property = propertyOf(state)
  if (!property || (pivot && canonical(pivot) === canonical(property))) return state
  const dimensions: Field[] = pivot ? [property, pivot] : [property]
  const kept = new Set([...state.measures, ...dimensions].map(canonical))
  const sort = state.sort.filter((s) => kept.has(canonical(s.by)))
  return { ...state, dimensions, sort, page: { ...state.page, index: 0 } }
}

/** Count, sum, mean, minimum and maximum of a numeric event property, as one panel without dimensions. */
export function statsPanel(state: ExplorationState, types: PropertyType[] | null): Panel[] {
  const field = propertyOf(state)
  if (!field || field.scope !== 'event' || !types || !isNumeric(types)) return []
  return [
    {
      measures: (['count', 'sum', 'mean', 'min', 'max'] as const).map((kind) => stat(kind, field)),
      dimensions: [],
      sort: [],
      page: { size: 1, index: 0 },
    },
  ]
}
