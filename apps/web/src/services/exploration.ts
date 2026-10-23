import type {
  Cell,
  Compare,
  Comparison,
  Condition,
  ExplorationState,
  Field,
  Granularity,
  Group,
  GroupOp,
  Measure,
  NamedMeasure,
  Node,
  Range,
  ResolvedRange,
  Sort,
  Value,
  Visualization,
} from '~/generated'
import {
  EXPLORATION_DIMENSIONS_MAX,
  EXPLORATION_PAGE_SIZE_DEFAULT,
  EXPLORATION_SERIES_POINTS_MAX,
  METRIC_VERSION,
  PAGE_SIZE_MAX,
  QUERY_STRING_MAX_BYTES,
  TYPICAL_WEEKDAY_RANGE_DAYS_MAX,
} from '~/generated/limits'

/**
 * Exploration behaviour without Vue: the one typed state of decision 0004, its canonical and
 * URL forms, and the operations a person applies to it. Every operation returns a new
 * normalized state; nothing here fetches or caches results.
 */

export const STATE_VERSION = 1

/** The overview's five measures and range, for a site without a state in the URL. */
export function defaultState(site: string, timezone: string): ExplorationState {
  return {
    v: STATE_VERSION,
    site,
    measures: ['pageviews', 'sessions', 'visitor_days', 'bounce_rate', 'span_mean'],
    dimensions: [],
    filter: null,
    range: { kind: 'relative', unit: 'day', count: 7 },
    timezone,
    granularity: 'day',
    compare: 'previous_period',
    metric_version: METRIC_VERSION,
    sort: [],
    page: { size: EXPLORATION_PAGE_SIZE_DEFAULT, index: 0 },
    visualization: 'bar',
  }
}

/**
 * The site overview without a state in the URL: today by the hour against a typical weekday
 * (0015 decision 4). States from URLs and saved views keep their own range and comparison.
 */
export function overviewState(site: string, timezone: string): ExplorationState {
  return {
    ...defaultState(site, timezone),
    range: { kind: 'relative', unit: 'day', count: 1 },
    granularity: 'hour',
    compare: 'typical_weekday',
  }
}

/** RFC 8785 style text: object keys sorted by UTF-16 code units, no whitespace. */
export function canonical(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
  const entries = Object.entries(value)
    .filter(([, item]) => item !== undefined)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
  return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonical(item)}`).join(',')}}`
}

function toBase64Url(text: string): string {
  let binary = ''
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function fromBase64Url(text: string): string | null {
  try {
    const binary = atob(text.replace(/-/g, '+').replace(/_/g, '/'))
    const bytes = Uint8Array.from(binary, (char) => char.charCodeAt(0))
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return null
  }
}

/** The `s` URL parameter: base64url of the canonical state. */
export function encodeState(state: ExplorationState): string {
  return toBase64Url(canonical(state))
}

export type Decoded =
  | { ok: true; state: ExplorationState }
  | { ok: false; reason: 'unreadable' | 'version' | 'oversize' }

/** Encoded states above this size are refused on load (0004 Limits). */
export const URL_STATE_MAX_BYTES = QUERY_STRING_MAX_BYTES

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const oneOf = <T extends string>(value: unknown, options: readonly T[]): value is T =>
  typeof value === 'string' && options.some((option) => option === value)

/**
 * The shape of a state read from an untrusted URL. Field types, operators and limits are the
 * server's to check; a state it refuses shows that refusal instead of another report.
 */
function isState(value: unknown): value is ExplorationState {
  if (!isRecord(value)) return false
  const { range, page, filter } = value
  return (
    value.v === STATE_VERSION &&
    typeof value.site === 'string' &&
    Array.isArray(value.measures) &&
    Array.isArray(value.dimensions) &&
    (filter === null || (isRecord(filter) && filter.kind === 'group' && Array.isArray(filter.items))) &&
    isRecord(range) &&
    oneOf(range.kind, ['relative', 'days', 'all'] as const) &&
    typeof value.timezone === 'string' &&
    oneOf(value.granularity, ['hour', 'day', 'week', 'month'] as const) &&
    oneOf(value.compare, ['none', 'previous_period', 'typical_weekday'] as const) &&
    typeof value.metric_version === 'number' &&
    Array.isArray(value.sort) &&
    isRecord(page) &&
    typeof page.size === 'number' &&
    typeof page.index === 'number' &&
    oneOf(value.visualization, ['table', 'line', 'bar', 'map', 'grid', 'funnel', 'journey', 'cohort'] as const)
  )
}

/** The JSON value of a base64url parameter, `undefined` when it is not one. */
function fromUrlJson(text: string): unknown {
  const json = fromBase64Url(text)
  if (json === null) return undefined
  try {
    return JSON.parse(json)
  } catch {
    return undefined
  }
}

export function decodeState(text: string): Decoded {
  if (text.length > URL_STATE_MAX_BYTES) return { ok: false, reason: 'oversize' }
  const parsed = fromUrlJson(text)
  if (isRecord(parsed) && parsed.v !== STATE_VERSION) return { ok: false, reason: 'version' }
  return isState(parsed) ? { ok: true, state: normalize(parsed) } : { ok: false, reason: 'unreadable' }
}

/**
 * A filter tree alone in a URL parameter, in the state's URL form: base64url of its canonical
 * text. The replay list takes it, so a finding links to the replays its filter matches.
 */
export const encodeFilter = (group: Group): string => toBase64Url(canonical(group))

export type DecodedFilter = { ok: true; filter: Group } | { ok: false; reason: 'unreadable' | 'oversize' }

const isGroup = (value: unknown): value is Group =>
  isRecord(value) && value.kind === 'group' && oneOf(value.op, ['and', 'or'] as const) && Array.isArray(value.items)

/** Like {@link decodeState}: the shape only, limits and fields are the server's to check. */
export function decodeFilter(text: string): DecodedFilter {
  if (text.length > URL_STATE_MAX_BYTES) return { ok: false, reason: 'oversize' }
  const parsed = fromUrlJson(text)
  const filter = isGroup(parsed) ? normalizeGroup(parsed) : null
  return filter ? { ok: true, filter } : { ok: false, reason: 'unreadable' }
}

function normalizeGroup(group: Group): Group | null {
  const items = group.items.flatMap((item): Node[] => {
    if (item.kind === 'cond') return [item]
    const inner = normalizeGroup(item)
    return inner ? [inner] : []
  })
  return items.length > 0 ? { ...group, items } : null
}

const unique = <T>(items: T[]): T[] =>
  items.filter((item, index) => items.findIndex((other) => canonical(other) === canonical(item)) === index)

/** Empty groups go, duplicates go, the page size stays within 1 to 100. */
export function normalize(state: ExplorationState): ExplorationState {
  return {
    ...state,
    measures: unique(state.measures),
    dimensions: unique(state.dimensions),
    filter: state.filter ? normalizeGroup(state.filter) : null,
    page: {
      size: Math.min(Math.max(1, Math.trunc(state.page.size)), PAGE_SIZE_MAX),
      index: Math.max(0, Math.trunc(state.page.index)),
    },
  }
}

/** Equal when their canonical forms are equal. */
export const sameState = (a: ExplorationState, b: ExplorationState) => canonical(a) === canonical(b)

/** Every condition of a filter tree, depth first. */
export function conditions(group: Group | null): Condition[] {
  if (!group) return []
  return group.items.flatMap((item) => (item.kind === 'cond' ? [item] : conditions(item)))
}

/**
 * Where a predicate tree lives in a state: the filter, a funnel step, a goal of a goals
 * report, or the goal of an attribution conversion (0004 Report definitions).
 */
export type TreePath = { kind: 'filter' } | { kind: 'step' | 'goal' | 'conversion'; index: number }

export const filterTree: TreePath = { kind: 'filter' }

/** Every predicate tree of a state: the filter, then the report's steps and goals. */
export function trees(state: ExplorationState): Group[] {
  const own = state.filter ? [state.filter] : []
  const report = state.report
  switch (report?.kind) {
    case 'funnel':
      return [...own, ...report.steps]
    case 'goals':
      return [...own, ...report.goals.map((goal) => goal.predicate)]
    case 'attribution':
      return [...own, ...report.conversions.flatMap((c) => (c.kind === 'goal' ? [c.goal.predicate] : []))]
    default:
      return own
  }
}

/** The tree at `path`, or `null` when the state has none there. */
export function treeAt(state: ExplorationState, path: TreePath): Group | null {
  const report = state.report
  switch (path.kind) {
    case 'filter':
      return state.filter
    case 'step':
      return report?.kind === 'funnel' ? (report.steps[path.index] ?? null) : null
    case 'goal':
      return report?.kind === 'goals' ? (report.goals[path.index]?.predicate ?? null) : null
    case 'conversion': {
      const conversion = report?.kind === 'attribution' ? report.conversions[path.index] : undefined
      return conversion?.kind === 'goal' ? conversion.goal.predicate : null
    }
  }
}

/**
 * Replaces the tree at `path`. An emptied filter becomes `null`; an emptied report tree stays
 * an empty group, which the server refuses until it gets a condition again.
 */
export function withTree(state: ExplorationState, path: TreePath, group: Group | null): ExplorationState {
  const empty: Group = { kind: 'group', op: group?.op ?? 'and', items: [] }
  const tree = group ?? empty
  const report = state.report
  const at = <T>(items: T[], index: number, f: (item: T) => T) => items.map((item, i) => (i === index ? f(item) : item))
  switch (path.kind) {
    case 'filter':
      return { ...state, filter: group && group.items.length > 0 ? group : null }
    case 'step':
      return report?.kind === 'funnel'
        ? { ...state, report: { ...report, steps: at(report.steps, path.index, () => tree) } }
        : state
    case 'goal':
      return report?.kind === 'goals'
        ? { ...state, report: { ...report, goals: at(report.goals, path.index, (goal) => ({ ...goal, predicate: tree })) } }
        : state
    case 'conversion':
      return report?.kind === 'attribution'
        ? {
            ...state,
            report: {
              ...report,
              conversions: at(report.conversions, path.index, (c) =>
                c.kind === 'goal' ? { ...c, goal: { ...c.goal, predicate: tree } } : c
              ),
            },
          }
        : state
  }
}

/**
 * A fresh predicate ID: `f` and one more than the highest `f` number in use in the filter and
 * every report tree, since IDs are unique across the state. IDs are never renumbered.
 */
export function nextId(state: ExplorationState): string {
  const used = trees(state).flatMap((tree) => conditions(tree).map((c) => /^f(\d+)$/.exec(c.id)?.[1]))
  const highest = Math.max(0, ...used.map((n) => (n ? Number(n) : 0)))
  return `f${highest + 1}`
}

/** A copy of a group with fresh IDs from `state` on, so it can join the state's trees. */
export function renumber(state: ExplorationState, group: Group): Group {
  let next = Number(nextId(state).slice(1))
  const copy = (inner: Group): Group => ({
    ...inner,
    items: inner.items.map((item) => (item.kind === 'group' ? copy(item) : { ...item, id: `f${next++}` })),
  })
  return copy(group)
}

const firstPage = (state: ExplorationState): ExplorationState => ({ ...state, page: { ...state.page, index: 0 } })

/** Appends a condition to the tree's root group, creating an AND root when there is none. */
export function addCondition(
  state: ExplorationState,
  condition: Omit<Condition, 'id' | 'kind'>,
  tree: TreePath = filterTree
): ExplorationState {
  const added: Condition = { kind: 'cond', id: nextId(state), ...condition }
  const root: Group = treeAt(state, tree) ?? { kind: 'group', op: 'and', items: [] }
  return firstPage(normalize(withTree(state, tree, { ...root, items: [...root.items, added] })))
}

function mapGroup(group: Group, f: (node: Node) => Node[]): Group {
  return {
    ...group,
    items: group.items.flatMap((item) => (item.kind === 'group' ? [mapGroup(item, f)] : f(item))),
  }
}

/** Empty nested groups go; an emptied root follows [`withTree`]. */
const pruned = (group: Group): Group => normalizeGroup(group) ?? { ...group, items: [] }

export function removeCondition(state: ExplorationState, id: string, tree: TreePath = filterTree): ExplorationState {
  const root = treeAt(state, tree)
  if (!root) return state
  const group = mapGroup(root, (node) => (node.kind === 'cond' && node.id === id ? [] : [node]))
  return firstPage(normalize(withTree(state, tree, pruned(group))))
}

/** Replaces a condition in place; its ID stays. */
export function replaceCondition(
  state: ExplorationState,
  id: string,
  condition: Omit<Condition, 'id' | 'kind'>,
  tree: TreePath = filterTree
): ExplorationState {
  const root = treeAt(state, tree)
  if (!root) return state
  const group = mapGroup(root, (node) =>
    node.kind === 'cond' && node.id === id ? [{ kind: 'cond', id, ...condition }] : [node]
  )
  return firstPage(normalize(withTree(state, tree, group)))
}

export const otherOp = (op: GroupOp): GroupOp => (op === 'and' ? 'or' : 'and')

/** Switches a group between AND and OR. `path` indexes nested groups from the root. */
export function setGroupOp(
  state: ExplorationState,
  path: number[],
  op: GroupOp,
  tree: TreePath = filterTree
): ExplorationState {
  const root = treeAt(state, tree)
  if (!root) return state
  const update = (group: Group, rest: number[]): Group => {
    if (rest.length === 0) return { ...group, op }
    const [head, ...tail] = rest
    return {
      ...group,
      items: group.items.map((item, index) => (index === head && item.kind === 'group' ? update(item, tail) : item)),
    }
  }
  return firstPage(withTree(state, tree, update(root, path)))
}

/**
 * Groups a root entry with the entry before it, under the operator opposite to the root's:
 * `a AND b` becomes `(a OR b)`. Joining a group extends it.
 */
export function groupWithPrevious(state: ExplorationState, index: number, tree: TreePath = filterTree): ExplorationState {
  const root = treeAt(state, tree)
  if (!root || index < 1 || index >= root.items.length) return state
  const previous = root.items[index - 1]
  const current = root.items[index]
  if (!previous || !current) return state
  const joined: Group =
    previous.kind === 'group' && previous.op !== root.op
      ? { ...previous, items: [...previous.items, current] }
      : { kind: 'group', op: otherOp(root.op), items: [previous, current] }
  const items = [...root.items.slice(0, index - 1), joined, ...root.items.slice(index + 1)]
  return firstPage(withTree(state, tree, { ...root, items }))
}

/** Lifts a nested group's entries back into the root. */
export function ungroup(state: ExplorationState, index: number, tree: TreePath = filterTree): ExplorationState {
  const root = treeAt(state, tree)
  const group = root?.items[index]
  if (!root || !group || group.kind !== 'group') return state
  const items = [...root.items.slice(0, index), ...group.items, ...root.items.slice(index + 1)]
  return firstPage(withTree(state, tree, { ...root, items }))
}

/** The typed value a cell stands for, or `null` for the `(null)` and not-set cells. */
export function cellValue(cell: Cell): Value | null {
  switch (cell.t) {
    case 'string':
      return { t: 'string', v: cell.v }
    case 'integer':
      return { t: 'integer', v: cell.v }
    case 'decimal':
      return { t: 'decimal', v: cell.v }
    case 'date':
      return { t: 'date', v: cell.v }
    case 'bool':
      return { t: 'bool', v: cell.v }
    case 'null':
    case 'not_set':
      return null
  }
}

/** Keeps only rows in a cell (0004: drill-downs append a condition with a new ID). */
export function include(state: ExplorationState, field: Field, cell: Cell): ExplorationState {
  if (cell.t === 'not_set') return addCondition(state, { field, cmp: 'is_not_set' })
  if (cell.t === 'null') return addCondition(state, { field, cmp: 'is_null' })
  return addCondition(state, { field, cmp: 'eq', value: cellValue(cell) ?? undefined })
}

/** Drops a cell's rows. The `(null)` cell has no complement operator, so it cannot be excluded. */
export function exclude(state: ExplorationState, field: Field, cell: Cell): ExplorationState | null {
  if (cell.t === 'null') return null
  if (cell.t === 'not_set') return addCondition(state, { field, cmp: 'is_set' })
  return addCondition(state, { field, cmp: 'neq', value: cellValue(cell) ?? undefined })
}

export const canSplit = (state: ExplorationState, field: Field) =>
  state.dimensions.length < EXPLORATION_DIMENSIONS_MAX &&
  !state.dimensions.some((d) => canonical(d) === canonical(field))

/** Adds a dimension; a table shows the split. */
export function splitBy(state: ExplorationState, field: Field): ExplorationState {
  if (!canSplit(state, field)) return state
  const visualization: Visualization = state.dimensions.length === 0 ? 'table' : state.visualization
  return firstPage({ ...state, dimensions: [...state.dimensions, field], visualization })
}

export function removeDimension(state: ExplorationState, field: Field): ExplorationState {
  const key = canonical(field)
  const dimensions = state.dimensions.filter((d) => canonical(d) !== key)
  const sort = state.sort.filter((s) => canonical(s.by) !== key)
  const visualization = state.visualization === 'grid' ? 'table' : state.visualization
  return firstPage({ ...state, dimensions, sort, visualization })
}

/** Removes every condition; dates, measures, dimensions and visualization stay (Kit contract). */
export const clearFilters = (state: ExplorationState, tree: TreePath = filterTree): ExplorationState =>
  firstPage(withTree(state, tree, null))

export const setRange = (state: ExplorationState, range: Range): ExplorationState => firstPage({ ...state, range })

export const setCompare = (state: ExplorationState, compare: Compare): ExplorationState => ({ ...state, compare })

/** Why a state cannot use a comparison, mirroring the server's `compare_unsupported` rules. */
export type CompareBlock =
  | { reason: 'range'; days: number }
  | { reason: 'measure'; measure: Measure }
  | { reason: 'report' }

/** Measures a typical-weekday baseline exists for (0015); money, percentiles and people have none. */
const baselined: readonly Measure[] = [
  'pageviews',
  'events',
  'visitors',
  'visitor_days',
  'sessions',
  'revenue_count',
  'bounce_rate',
  'span_mean',
  'span_median',
]

/** The most local days a range can cover; calendar weeks and months count in full. */
export function rangeDays(range: Range): number {
  if (range.kind === 'all') return Number.POSITIVE_INFINITY
  if (range.kind === 'days') return Math.round((Date.parse(range.end) - Date.parse(range.start)) / 86_400_000) + 1
  return range.count * { day: 1, week: 7, month: 31 }[range.unit]
}

/**
 * Whether the state can use a comparison, or why not. Typical weekday needs a range of at most
 * 31 days and baselined measures; reports compare only goals with a typical weekday.
 */
export function compareBlock(state: ExplorationState, compare: Compare): CompareBlock | null {
  if (compare === 'none') return null
  if (state.report) return compare === 'typical_weekday' && state.report.kind === 'goals' ? null : { reason: 'report' }
  if (compare === 'previous_period') return null
  const measure = state.measures.find((m) => !baselined.some((b) => canonical(b) === canonical(m)))
  if (measure) return { reason: 'measure', measure }
  const days = rangeDays(state.range)
  return days > TYPICAL_WEEKDAY_RANGE_DAYS_MAX ? { reason: 'range', days: TYPICAL_WEEKDAY_RANGE_DAYS_MAX } : null
}

export type Weekday = 1 | 2 | 3 | 4 | 5 | 6 | 7

/** What a comparison is against: one weekday, the weekdays of a longer range, or the previous period. */
export type Baseline = { kind: 'weekday'; weekday: Weekday } | { kind: 'weekdays' } | { kind: 'previous' }

const weekdayIndex = { Mon: 1, Tue: 2, Wed: 3, Thu: 4, Fri: 5, Sat: 6, Sun: 7 } as const satisfies Record<
  string,
  Weekday
>
const isWeekdayName = (name: string): name is keyof typeof weekdayIndex => name in weekdayIndex

/** ISO weekday (Monday 1) of an instant in a timezone. */
export function weekdayOf(ms: number, timezone: string): Weekday {
  const name = new Intl.DateTimeFormat('en-US', { timeZone: timezone, weekday: 'short' }).format(ms)
  return isWeekdayName(name) ? weekdayIndex[name] : 1
}

/** Local date `YYYY-MM-DD` of an instant in a timezone. */
export const localDate = (ms: number, timezone: string) =>
  new Intl.DateTimeFormat('en-CA', { timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(ms)

/** Whether a resolved range covers a single local day. */
export const isSingleDay = (resolved: ResolvedRange) =>
  localDate(resolved.start, resolved.timezone) === localDate(Math.max(resolved.start, resolved.end - 1), resolved.timezone)

export const containsNow = (resolved: ResolvedRange, now: number) => resolved.start <= now && now < resolved.end

/** What `compare` is against for this range, or `null` without a comparison. */
export function baselineOf(compare: Compare, resolved: ResolvedRange): Baseline | null {
  switch (compare) {
    case 'none':
      return null
    case 'previous_period':
      return { kind: 'previous' }
    case 'typical_weekday':
      return isSingleDay(resolved) ? { kind: 'weekday', weekday: weekdayOf(resolved.start, resolved.timezone) } : { kind: 'weekdays' }
  }
}

/**
 * Sets the range from the scope chip and drops what the new range cannot use, in the same one
 * state: hourly buckets past the series limit become daily, and a typical weekday past 31 days
 * becomes the previous period. Explicit choices on a range that fits stay as they are.
 */
export function fitRange(state: ExplorationState, range: Range): ExplorationState {
  const next = setRange(state, range)
  const hours = rangeDays(range) * 24
  const granularity = next.granularity === 'hour' && hours > EXPLORATION_SERIES_POINTS_MAX ? 'day' : next.granularity
  const compare = compareBlock(next, 'typical_weekday')?.reason === 'range' && next.compare === 'typical_weekday' ? 'previous_period' : next.compare
  return { ...next, granularity, compare }
}

/** Within this relative change a value reads "about the same as" its comparison. */
export const SAME_CHANGE_MAX = 0.05

/**
 * How a value compares, in the words the headline and the measures strip use: above or below
 * with the relative change, about the same within ±5 % or when both sides are 0, `new` against
 * a zero baseline (0003), or `null` when either side is unavailable.
 */
export type ChangeWords =
  | { kind: 'above' | 'below'; ratio: number }
  | { kind: 'same' }
  | { kind: 'new' }

export function changeWords(comparison: Comparison | undefined): ChangeWords | null {
  if (!comparison) return null
  const ratio = comparison.relative_change
  if (ratio === undefined) {
    if (comparison.reason !== 'zero_baseline' || comparison.change === undefined) return null
    return comparison.change > 0 ? { kind: 'new' } : { kind: 'same' }
  }
  if (Math.abs(ratio) <= SAME_CHANGE_MAX) return { kind: 'same' }
  return { kind: ratio > 0 ? 'above' : 'below', ratio: Math.abs(ratio) }
}

/** Measures where a drop is the good direction: bounces, refunds and load times. */
const lowerIsBetter: ReadonlySet<Measure> = new Set<NamedMeasure>(['bounce_rate', 'revenue_refunds', 'p50', 'p75', 'p95'])

/**
 * Whether a change is good or bad news for `measure`, so it can take the success or destructive
 * tone. Within ±5 % it is `neutral`, as is any change of a property statistic, whose direction
 * means nothing on its own.
 */
export type ChangeTone = 'good' | 'bad' | 'neutral'

export function changeTone(measure: Measure, comparison: Comparison | undefined): ChangeTone {
  const words = changeWords(comparison)
  if (typeof measure !== 'string' || !words || words.kind === 'same') return 'neutral'
  const rising = words.kind !== 'below'
  return rising !== lowerIsBetter.has(measure) ? 'good' : 'bad'
}

export const setGranularity = (state: ExplorationState, granularity: Granularity): ExplorationState => ({
  ...state,
  granularity,
})

export const setVisualization = (state: ExplorationState, visualization: Visualization): ExplorationState => ({
  ...state,
  visualization,
})

/** Sets the measures; sort keys on a removed measure go with it. */
export function setMeasures(state: ExplorationState, measures: Measure[]): ExplorationState {
  if (measures.length === 0) return state
  const kept = new Set(measures.map(canonical))
  const sort = state.sort.filter((s) => kept.has(canonical(s.by)) || isField(s.by, state))
  return firstPage(normalize({ ...state, measures, sort }))
}

const isField = (key: Sort['by'], state: ExplorationState) =>
  state.dimensions.some((d) => canonical(d) === canonical(key))

/** Sorting by one key: a second click on the same key flips its direction. */
export function toggleSort(state: ExplorationState, by: Sort['by']): ExplorationState {
  const current = state.sort[0]
  const dir = current && canonical(current.by) === canonical(by) && current.dir === 'desc' ? 'asc' : 'desc'
  return firstPage({ ...state, sort: [{ by, dir }] })
}

export const setPage = (state: ExplorationState, index: number): ExplorationState => ({
  ...state,
  page: { ...state.page, index: Math.max(0, index) },
})

/** Copies a segment's conditions into the filter with fresh IDs; the state never references it. */
export function applySegment(state: ExplorationState, segment: Group): ExplorationState {
  const copy = renumber(state, segment)
  const root: Group = state.filter ?? { kind: 'group', op: 'and', items: [] }
  const items = copy.op === root.op ? [...root.items, ...copy.items] : [...root.items, copy]
  return firstPage(normalize({ ...state, filter: { ...root, items } }))
}

/** Fits in the URL limit, including the parameter name. */
export const fitsUrl = (state: ExplorationState) => encodeState(state).length + 2 <= URL_STATE_MAX_BYTES
