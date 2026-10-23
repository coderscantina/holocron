import type { Annotation, Compare, Comparison, Finding, Group, Metric, MoverValues, Projection, Range, ResolvedRange } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'
import {
  baselineOf,
  changeWords,
  containsNow,
  isSingleDay,
  type Baseline,
  type ChangeWords,
  type Weekday,
} from '~/services/exploration'

/**
 * The overview headline's grammar (design contract, Headline): which fixed sentences describe a
 * result, chosen from the result's values alone. Every sentence is one message; the Vue side
 * fills its placeholders with links and buttons. Nothing here formats numbers or translates.
 */

export type Estimate = Extract<Projection, { t: 'estimate' }>

export interface HeadlineInput {
  total: Metric
  comparison: Comparison | undefined
  projection: Projection | null | undefined
  compare: Compare
  range: Range
  resolved: ResolvedRange
  /** The application clock, Unix milliseconds. */
  now: number
}

/** A message and the plain values of its placeholders. */
export interface Phrase {
  key: MessageKey
  named: Record<string, string | number>
}

export interface Headline {
  /** First sentence, primary text. Placeholders: `value`, `measure`, `range`, and `change` when compared. */
  lead: MessageKey
  /** The pace sentence when there is an estimate. Placeholders: `projection`, `change`. */
  pace: MessageKey | null
  /** The comparison clause. Placeholders: `percent` (a ratio, unsigned) and `weekday` (a name). */
  change: { key: MessageKey; ratio: number | null; weekday: Weekday | null } | null
  /** The range as the sentence names it ("today", "in the last 7 days"). */
  range: Phrase
  estimate: Estimate | null
  /** Where the estimate comes from, for its tooltip. Placeholder: `count`. */
  basis: MessageKey | null
  /** Muted sentences after the headline. */
  notes: MessageKey[]
}

function rangePhrase(range: Range): Phrase {
  if (range.kind === 'all') return { key: 'overview.headline.range.all', named: {} }
  if (range.kind === 'days') {
    return range.start === range.end
      ? { key: 'overview.headline.range.day', named: { date: range.start } }
      : { key: 'overview.headline.range.fixed', named: { start: range.start, end: range.end } }
  }
  if (range.unit === 'day' && range.count === 1) return { key: 'overview.headline.range.today', named: {} }
  const key = (
    { day: 'overview.headline.range.days', week: 'overview.headline.range.weeks', month: 'overview.headline.range.months' } as const
  )[range.unit]
  return { key, named: { count: range.count } }
}

const changeKey = (baseline: Baseline, words: ChangeWords): MessageKey =>
  `overview.headline.change.${baseline.kind}.${words.kind}`

const basisKey = (baseline: Baseline): MessageKey =>
  baseline.kind === 'weekday' ? `overview.headline.basis.weekday.${baseline.weekday}` : 'overview.headline.basis.days'

/**
 * The sentences for one measure of a result, by the contract's grammar table:
 * range containing now with an estimate, the same without one, a past range with a comparison,
 * and no comparison. A typical weekday without enough history reads as no comparison.
 */
export function headline(input: HeadlineInput): Headline {
  const { resolved } = input
  const now = containsNow(resolved, input.now)
  const baseline = baselineOf(input.compare, resolved)
  const words = baseline ? changeWords(input.comparison) : null
  const estimate = now && input.projection?.t === 'estimate' ? input.projection : null
  const notes: MessageKey[] = []

  if (input.compare === 'typical_weekday' && input.comparison?.reason === 'insufficient_history') {
    notes.push('overview.headline.notes.noTypicalDay')
  } else if (words && now && !estimate && input.projection?.t === 'unavailable' && input.projection.reason === 'insufficient_history') {
    notes.push('overview.headline.notes.noEstimate')
  }

  const range = rangePhrase(input.range)
  if (!baseline || !words) {
    return { lead: 'overview.headline.lead.plain', pace: null, change: null, range, estimate: null, basis: null, notes }
  }

  const change = {
    key: changeKey(baseline, words),
    ratio: words.kind === 'above' || words.kind === 'below' ? words.ratio : null,
    weekday: baseline.kind === 'weekday' ? baseline.weekday : null,
  }
  if (estimate) {
    return {
      lead: 'overview.headline.lead.soFar',
      pace: isSingleDay(resolved) ? 'overview.headline.pace.midnight' : 'overview.headline.pace.endOfToday',
      change,
      range,
      estimate,
      basis: basisKey(baseline),
      notes,
    }
  }
  return {
    lead: now ? 'overview.headline.lead.soFarChange' : 'overview.headline.lead.past',
    pace: null,
    change,
    range,
    estimate: null,
    basis: null,
    notes,
  }
}

export interface WhyInput {
  /** The site's findings in rank order. */
  findings: readonly Finding[]
  /** The summary is stale or more than 30 minutes behind. */
  stale: boolean
  /** Annotations of the range on screen. */
  annotations: readonly Annotation[]
  /** The headline has a comparison clause. */
  compared: boolean
  resolved: ResolvedRange
  /** The application clock, Unix milliseconds. */
  now: number
}

/** The share of the site's change a mover must explain to be named, 0 to 1. */
export const WHY_SHARE_MIN = 0.5

export type Why =
  | { kind: 'usual'; key: MessageKey }
  | {
      kind: 'mover'
      /** Placeholders: `value`, `change`, `time`, and `annotation` for the annotated sentences. */
      key: MessageKey
      mover: MoverValues
      filter: Group | null
      /** The mover's change in visitor-days, signed. */
      change: number
      /** Start of the finding's window, Unix milliseconds. */
      since: number
      annotation: Annotation | null
    }

/**
 * The line under the headline (design contract, Headline): the top dimension mover that
 * explains at least half of today's change, with the first annotation inside its window.
 * Findings measure today so far against a typical weekday, so the line only describes a range
 * that is today, and only when the headline has a comparison. Stale findings say nothing.
 */
export function why(input: WhyInput): Why | null {
  if (input.stale || !input.compared) return null
  if (!containsNow(input.resolved, input.now) || !isSingleDay(input.resolved)) return null
  for (const finding of input.findings) {
    const values = finding.values
    if (values.kind !== 'dimension_mover' || finding.share < WHY_SHARE_MIN) continue
    const change = values.today - values.typical
    const annotation =
      input.annotations
        .filter((note) => note.at >= finding.window_start && note.at <= finding.computed_at)
        .reduce<Annotation | null>((first, note) => (first && first.at <= note.at ? first : note), null)
    const direction = change >= 0 ? 'lift' : 'drop'
    return {
      kind: 'mover',
      key: annotation ? `overview.headline.why.${direction}After` : `overview.headline.why.${direction}`,
      mover: values,
      filter: finding.state.filter,
      change,
      since: finding.window_start,
      annotation,
    }
  }
  return { kind: 'usual', key: 'overview.headline.why.usual' }
}
