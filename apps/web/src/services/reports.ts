import type {
  AttributionReport,
  Cell,
  Condition,
  Conversion,
  ExplorationState,
  Field,
  FunnelReport,
  GoalDefinition,
  Group,
  JourneyReport,
  ReportDefinition,
  SavedGoal,
  SessionColumn,
  Visualization,
} from '~/generated'
import {
  REPORT_ATTRIBUTION_LOOKBACK_DAYS,
  REPORT_FUNNEL_STEPS_MAX,
  REPORT_GOALS_MAX,
  REPORT_ATTRIBUTION_CONVERSIONS_MAX,
} from '~/generated/limits'

import { conditions, defaultState, include, nextId, renumber } from './exploration'

/**
 * The behavioral report states of milestone 07 (0004 Report definitions): funnels, goals,
 * journeys, attribution and session lists, the edits their builders make, and drill-downs into
 * the sessions or events behind a result. Like the exploration service, every operation
 * returns a new state and nothing here fetches.
 */

export const REPORT_VERSION = 1
/** Funnels that split by a field; without a split they stay version 1, so saved states keep their URLs. */
const FUNNEL_SPLIT_VERSION = 2

/** A report state over the last 30 days: no measures, dimensions, comparison or sort. */
function reportState(
  site: string,
  timezone: string,
  report: ReportDefinition,
  visualization: Visualization
): ExplorationState {
  return {
    ...defaultState(site, timezone),
    measures: [],
    dimensions: [],
    range: { kind: 'relative', unit: 'day', count: 30 },
    compare: 'none',
    visualization,
    report,
  }
}

export type NewCondition = Omit<Condition, 'id' | 'kind'>

/** A one-condition group with a fresh ID from `state`. */
const single = (state: ExplorationState, condition: NewCondition): Group => ({
  kind: 'group',
  op: 'and',
  items: [{ kind: 'cond', id: nextId(state), ...condition }],
})

/** Pageviews of a path: custom events carry their page's path too, so they need no event name. */
export const pageview = (path: string): NewCondition[] => [
  { field: 'path', cmp: 'eq', value: { t: 'string', v: path } },
  { field: 'event_name', cmp: 'is_not_set' },
]

/** A group of conditions with fresh IDs from `state`. */
export function groupOf(state: ExplorationState, items: NewCondition[]): Group {
  const draft: Group = { kind: 'group', op: 'and', items: items.map((c, i) => ({ kind: 'cond', id: `x${i}`, ...c })) }
  return renumber(state, draft)
}

export function funnelState(site: string, timezone: string): ExplorationState {
  const empty = reportState(site, timezone, { kind: 'funnel', v: REPORT_VERSION, steps: [], window_seconds: 3600, scope: 'daily' }, 'funnel')
  // Any pageview, then any custom event: a funnel every site has data for, to refine from.
  const first = groupOf(empty, [{ field: 'event_name', cmp: 'is_not_set' }])
  const withFirst = setReport(empty, { kind: 'funnel', v: REPORT_VERSION, steps: [first], window_seconds: 3600, scope: 'daily' })
  return addStep(withFirst, [{ field: 'event_name', cmp: 'is_set' }])
}

export function journeyState(site: string, timezone: string): ExplorationState {
  return reportState(site, timezone, { kind: 'journey', v: REPORT_VERSION, value: 'path', length: 4, scope: 'session' }, 'journey')
}

export function attributionState(site: string, timezone: string): ExplorationState {
  return reportState(
    site,
    timezone,
    {
      kind: 'attribution',
      v: REPORT_VERSION,
      model: 'last_touch',
      lookback_days: REPORT_ATTRIBUTION_LOOKBACK_DAYS,
      scope: 'daily',
      touch_field: 'channel',
      conversions: [{ kind: 'revenue' }],
    },
    'table'
  )
}

/** Goals of a site, the first eight active ones as they are saved now. */
export function goalsState(site: string, timezone: string, saved: SavedGoal[]): ExplorationState {
  const empty = reportState(site, timezone, { kind: 'goals', v: REPORT_VERSION, goals: [] }, 'table')
  return saved.slice(0, REPORT_GOALS_MAX).reduce(addGoal, empty)
}

export const sessionColumns: SessionColumn[] = [
  'session_id',
  'start',
  'entry_path',
  'channel',
  'referrer_domain',
  'country',
  'city',
  'device',
  'browser',
  'os',
  'span',
  'pageviews',
  'custom_events',
  'identity_kind',
]

export function sessionListState(site: string, timezone: string): ExplorationState {
  return reportState(
    site,
    timezone,
    { kind: 'session_list', v: REPORT_VERSION, columns: sessionColumns, order: 'start_desc' },
    'table'
  )
}

const setReport = (state: ExplorationState, report: ReportDefinition): ExplorationState => ({ ...state, report })

/** The funnel of a state, for builders that only edit funnels. */
export const funnelOf = (state: ExplorationState): FunnelReport | null =>
  state.report?.kind === 'funnel' ? state.report : null

export const journeyOf = (state: ExplorationState): JourneyReport | null =>
  state.report?.kind === 'journey' ? state.report : null

export const attributionOf = (state: ExplorationState): AttributionReport | null =>
  state.report?.kind === 'attribution' ? state.report : null

export const goalsOf = (state: ExplorationState): GoalDefinition[] =>
  state.report?.kind === 'goals' ? state.report.goals : []

export function setFunnel(state: ExplorationState, patch: Partial<Pick<FunnelReport, 'window_seconds' | 'scope'>>) {
  const funnel = funnelOf(state)
  return funnel ? setReport(state, { kind: 'funnel', ...funnel, ...patch }) : state
}

/** Splits the funnel by a field (version 2), or drops the split (back to version 1). */
export function setSplit(state: ExplorationState, field: Field | null): ExplorationState {
  const funnel = funnelOf(state)
  if (!funnel) return state
  return setReport(state, {
    kind: 'funnel',
    ...funnel,
    v: field ? FUNNEL_SPLIT_VERSION : REPORT_VERSION,
    group_by: field ?? undefined,
  })
}

/**
 * The funnel of one group of a split: the same steps without the split, filtered to the group's
 * value on the split field, so every figure of the read is exact for that group. A split
 * funnel's own state is unchanged; the filter is a new condition with a fresh predicate ID.
 */
export function pickGroup(state: ExplorationState, cell: Cell): ExplorationState {
  const field = funnelOf(state)?.group_by
  return field ? include(setSplit(state, null), field, cell) : state
}

/** Appends a step matching all `items`; at most eight. */
export function addStep(state: ExplorationState, items: NewCondition[]): ExplorationState {
  const funnel = funnelOf(state)
  if (!funnel || funnel.steps.length >= REPORT_FUNNEL_STEPS_MAX) return state
  return setReport(state, { kind: 'funnel', ...funnel, steps: [...funnel.steps, groupOf(state, items)] })
}

export function removeStep(state: ExplorationState, index: number): ExplorationState {
  const funnel = funnelOf(state)
  if (!funnel) return state
  return setReport(state, { kind: 'funnel', ...funnel, steps: funnel.steps.filter((_, i) => i !== index) })
}

/** Moves a step one place up (`-1`) or down (`1`). */
export function moveStep(state: ExplorationState, index: number, delta: -1 | 1): ExplorationState {
  const funnel = funnelOf(state)
  const target = index + delta
  if (!funnel || target < 0 || target >= funnel.steps.length) return state
  const steps = [...funnel.steps]
  const [moved] = steps.splice(index, 1)
  if (!moved) return state
  steps.splice(target, 0, moved)
  return setReport(state, { kind: 'funnel', ...funnel, steps })
}

/** Whether every report tree has a condition, so the server can compute the state. */
export const complete = (state: ExplorationState): boolean => {
  const report = state.report
  if (report?.kind === 'funnel') return report.steps.every((step) => conditions(step).length > 0)
  if (report?.kind === 'goals') return report.goals.length > 0 && report.goals.every((g) => conditions(g.predicate).length > 0)
  if (report?.kind === 'attribution')
    return report.conversions.every((c) => c.kind === 'revenue' || conditions(c.goal.predicate).length > 0)
  return true
}

export function setJourney(state: ExplorationState, patch: Partial<Pick<JourneyReport, 'value' | 'length' | 'scope'>>) {
  const journey = journeyOf(state)
  return journey ? setReport(state, { kind: 'journey', ...journey, ...patch }) : state
}

export function setAttribution(
  state: ExplorationState,
  patch: Partial<Pick<AttributionReport, 'model' | 'scope' | 'touch_field'>>
): ExplorationState {
  const attribution = attributionOf(state)
  return attribution ? setReport(state, { kind: 'attribution', ...attribution, ...patch }) : state
}

/** A saved goal as a report stores it: inline, with where it came from (0004 Report definitions). */
export const inline = (goal: SavedGoal): GoalDefinition => ({
  ...goal.definition,
  ref: { id: goal.id, version: goal.version },
})

/** A goal's copy with predicate IDs that are fresh in `state`. */
const fresh = (state: ExplorationState, goal: GoalDefinition): GoalDefinition => ({
  ...goal,
  predicate: renumber(state, goal.predicate),
})

export function addConversion(state: ExplorationState, conversion: Conversion): ExplorationState {
  const attribution = attributionOf(state)
  if (!attribution || attribution.conversions.length >= REPORT_ATTRIBUTION_CONVERSIONS_MAX) return state
  const added: Conversion = conversion.kind === 'goal' ? { kind: 'goal', goal: fresh(state, conversion.goal) } : conversion
  return setReport(state, { kind: 'attribution', ...attribution, conversions: [...attribution.conversions, added] })
}

export function removeConversion(state: ExplorationState, index: number): ExplorationState {
  const attribution = attributionOf(state)
  if (!attribution || attribution.conversions.length <= 1) return state
  return setReport(state, {
    kind: 'attribution',
    ...attribution,
    conversions: attribution.conversions.filter((_, i) => i !== index),
  })
}

/** Replaces a conversion's goal with another version of it, keeping its place. */
export function replaceConversionGoal(state: ExplorationState, index: number, goal: GoalDefinition): ExplorationState {
  const attribution = attributionOf(state)
  if (!attribution) return state
  const replaced = fresh(state, goal)
  return setReport(state, {
    kind: 'attribution',
    ...attribution,
    conversions: attribution.conversions.map((c, i) => (i === index ? { kind: 'goal', goal: replaced } : c)),
  })
}

export function addGoal(state: ExplorationState, goal: SavedGoal | GoalDefinition): ExplorationState {
  const goals = goalsOf(state)
  if (state.report?.kind !== 'goals' || goals.length >= REPORT_GOALS_MAX) return state
  const definition = 'definition' in goal ? inline(goal) : goal
  return setReport(state, { kind: 'goals', v: REPORT_VERSION, goals: [...goals, fresh(state, definition)] })
}

export function removeGoal(state: ExplorationState, index: number): ExplorationState {
  return setReport(state, { kind: 'goals', v: REPORT_VERSION, goals: goalsOf(state).filter((_, i) => i !== index) })
}

/** Replaces a goal with another version of it, keeping its place in the report. */
export function replaceGoal(state: ExplorationState, index: number, goal: SavedGoal): ExplorationState {
  const replaced = fresh(state, inline(goal))
  return setReport(state, {
    kind: 'goals',
    v: REPORT_VERSION,
    goals: goalsOf(state).map((g, i) => (i === index ? replaced : g)),
  })
}

/** The newer saved version a report's goal could move to, if its saved goal has one. */
export function upgradeFor(goal: GoalDefinition, saved: SavedGoal[]): SavedGoal | null {
  const ref = goal.ref
  if (!ref) return null
  const current = saved.find((s) => s.id === ref.id)
  return current && current.version > ref.version ? current : null
}

/** The state's filter and `extra` trees joined with AND. IDs are already unique in one state. */
function joined(state: ExplorationState, extra: Group[]): Group | null {
  const own = state.filter ? (state.filter.op === 'and' ? state.filter.items : [state.filter]) : []
  const added = extra.flatMap((group) => (group.op === 'and' ? group.items : [group]))
  const items = [...own, ...added]
  return items.length > 0 ? { kind: 'group', op: 'and', items } : null
}

/**
 * The sessions behind a result: a session list over the same site, range and timezone, keeping
 * sessions with at least one event matching the filter and every `extra` tree.
 */
export function sessionsFor(state: ExplorationState, extra: Group[] = []): ExplorationState {
  return { ...sessionListState(state.site, state.timezone), range: state.range, filter: joined(state, extra) }
}

/** The events behind a result in Explore: matching events by page, with the same range. */
export function eventsFor(state: ExplorationState, extra: Group[] = []): ExplorationState {
  return {
    ...defaultState(state.site, state.timezone),
    measures: ['events', 'pageviews', 'visitor_days'],
    dimensions: ['path'],
    sort: [{ by: 'events', dir: 'desc' }],
    compare: 'none',
    visualization: 'table',
    range: state.range,
    filter: joined(state, extra),
  }
}

/** A condition on one session field's value, as a drill-down appends it. */
export const fieldEquals = (state: ExplorationState, field: Condition['field'], value: string | null): Group =>
  single(state, value === null ? { field, cmp: 'is_not_set' } : { field, cmp: 'eq', value: { t: 'string', v: value } })
