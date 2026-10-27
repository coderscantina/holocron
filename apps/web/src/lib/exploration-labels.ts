import type {
  DimensionField,
  Granularity,
  NamedMeasure,
  Operator,
  PropertyType,
  Range,
  Retention,
  Stat,
  Visualization,
} from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

import { coversMonths } from './retention'

/** Exhaustive label maps for the generated exploration enums. */

export const fieldLabels = {
  path: 'explore.fields.path',
  hostname: 'explore.fields.hostname',
  title: 'explore.fields.title',
  query: 'explore.fields.query',
  event_name: 'explore.fields.event_name',
  tag: 'explore.fields.tag',
  perf_metric: 'explore.fields.perf_metric',
  currency: 'explore.fields.currency',
  referrer_domain: 'explore.fields.referrer_domain',
  referrer_path: 'explore.fields.referrer_path',
  utm_source: 'explore.fields.utm_source',
  utm_medium: 'explore.fields.utm_medium',
  utm_campaign: 'explore.fields.utm_campaign',
  utm_content: 'explore.fields.utm_content',
  utm_term: 'explore.fields.utm_term',
  click_id: 'explore.fields.click_id',
  channel: 'explore.fields.channel',
  country: 'explore.fields.country',
  region: 'explore.fields.region',
  city: 'explore.fields.city',
  browser: 'explore.fields.browser',
  os: 'explore.fields.os',
  device: 'explore.fields.device',
  language: 'explore.fields.language',
  screen: 'explore.fields.screen',
  weekday: 'explore.fields.weekday',
  hour: 'explore.fields.hour',
} as const satisfies Record<DimensionField, MessageKey>

/** Built-in fields in the order the pickers list them. */
export const dimensionFields: DimensionField[] = [
  'path',
  'event_name',
  'currency',
  'referrer_domain',
  'channel',
  'country',
  'device',
  'browser',
  'os',
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_content',
  'utm_term',
  'title',
  'hostname',
  'query',
  'referrer_path',
  'click_id',
  'region',
  'city',
  'language',
  'screen',
  'tag',
  'weekday',
  'hour',
]

/** Fields compared as integers rather than text. */
export const integerFields: DimensionField[] = ['weekday', 'hour']

export const measureLabels = {
  pageviews: 'explore.measures.pageviews',
  events: 'explore.measures.events',
  visitors: 'explore.measures.visitors',
  visitor_days: 'explore.measures.visitor_days',
  identified_people: 'explore.measures.identified_people',
  sessions: 'explore.measures.sessions',
  bounce_rate: 'explore.measures.bounce_rate',
  span_mean: 'explore.measures.span_mean',
  span_median: 'explore.measures.span_median',
  revenue_net: 'explore.measures.revenue_net',
  revenue_gross: 'explore.measures.revenue_gross',
  revenue_refunds: 'explore.measures.revenue_refunds',
  revenue_count: 'explore.measures.revenue_count',
  p50: 'explore.measures.p50',
  p75: 'explore.measures.p75',
  p95: 'explore.measures.p95',
} as const satisfies Record<NamedMeasure, MessageKey>

export const namedMeasures: NamedMeasure[] = [
  'pageviews',
  'events',
  'visitor_days',
  'sessions',
  'bounce_rate',
  'span_mean',
  'span_median',
  'identified_people',
]

export const statLabels = {
  count: 'explore.stats.count',
  sum: 'explore.stats.sum',
  min: 'explore.stats.min',
  max: 'explore.stats.max',
  mean: 'explore.stats.mean',
} as const satisfies Record<Stat, MessageKey>

export const operatorLabels = {
  eq: 'explore.operators.eq',
  neq: 'explore.operators.neq',
  contains: 'explore.operators.contains',
  not_contains: 'explore.operators.not_contains',
  starts_with: 'explore.operators.starts_with',
  in: 'explore.operators.in',
  not_in: 'explore.operators.not_in',
  gt: 'explore.operators.gt',
  gte: 'explore.operators.gte',
  lt: 'explore.operators.lt',
  lte: 'explore.operators.lte',
  between: 'explore.operators.between',
  is_set: 'explore.operators.is_set',
  is_not_set: 'explore.operators.is_not_set',
  is_null: 'explore.operators.is_null',
  matches: 'explore.operators.matches',
  not_matches: 'explore.operators.not_matches',
} as const satisfies Record<Operator, MessageKey>

export const typeLabels = {
  null: 'explore.types.null',
  bool: 'explore.types.bool',
  integer: 'explore.types.integer',
  decimal: 'explore.types.decimal',
  date: 'explore.types.date',
  string: 'explore.types.string',
  array: 'explore.types.array',
} as const satisfies Record<PropertyType, MessageKey>

/** Every series interval, shortest first, as the chart's interval control offers them. */
export const granularities = ['hour', 'day', 'week', 'month'] as const satisfies readonly Granularity[]

export const granularityLabels = {
  hour: 'explore.granularity.hour',
  day: 'explore.granularity.day',
  week: 'explore.granularity.week',
  month: 'explore.granularity.month',
} as const satisfies Record<Granularity, MessageKey>

/** The visualizations this milestone draws; report kinds arrive with their milestones. */
export type OfferedVisualization = Extract<Visualization, 'table' | 'line' | 'bar' | 'grid'>

export const visualizationLabels = {
  table: 'explore.visualization.table',
  line: 'explore.visualization.line',
  bar: 'explore.visualization.bar',
  grid: 'explore.visualization.grid',
} as const satisfies Record<OfferedVisualization, MessageKey>

export const weekdayLabels = {
  1: 'explore.weekdays.1',
  2: 'explore.weekdays.2',
  3: 'explore.weekdays.3',
  4: 'explore.weekdays.4',
  5: 'explore.weekdays.5',
  6: 'explore.weekdays.6',
  7: 'explore.weekdays.7',
} as const satisfies Record<number, MessageKey>

/** Relative ranges every site offers; a share viewer is limited to these. */
export const rangePresets: Array<{ key: MessageKey; range: Range }> = [
  { key: 'explore.range.today', range: { kind: 'relative', unit: 'day', count: 1 } },
  { key: 'explore.range.last7', range: { kind: 'relative', unit: 'day', count: 7 } },
  { key: 'explore.range.last30', range: { kind: 'relative', unit: 'day', count: 30 } },
  { key: 'explore.range.last90', range: { kind: 'relative', unit: 'day', count: 90 } },
  { key: 'explore.range.last12Months', range: { kind: 'relative', unit: 'month', count: 12 } },
]

const longRangePresets: typeof rangePresets = [
  { key: 'explore.range.last3Years', range: { kind: 'relative', unit: 'month', count: 36 } },
  { key: 'explore.range.last5Years', range: { kind: 'relative', unit: 'month', count: 60 } },
]

/** The presets plus the longer ones a site's retention allows, and all retained time (decision 0016). */
export const rangePresetsFor = (retention: Retention): typeof rangePresets => [
  ...rangePresets,
  ...longRangePresets.filter(({ range }) => range.kind === 'relative' && coversMonths(retention, range.count)),
  { key: 'explore.range.all', range: { kind: 'all' } },
]
