import { afterEach, describe, expect, it } from 'vitest'

import type { Annotation, Comparison, Finding, Metric, Projection, Range, ResolvedRange } from '~/generated'
import { composer, type LocaleCode } from '~/plugins/i18n'

import { headline, why, type Headline, type HeadlineInput, type Why, type WhyInput } from './headline'

/** Tuesday 13 October 2026 in Vienna (UTC+2), at 15:20 local time. */
const tuesday: ResolvedRange = {
  start: Date.UTC(2026, 9, 12, 22),
  end: Date.UTC(2026, 9, 13, 22),
  compare_days: ['2026-08-18', '2026-08-25', '2026-09-01', '2026-09-08', '2026-09-15', '2026-09-22', '2026-09-29', '2026-10-06'],
  timezone: 'Europe/Vienna',
}
const now = Date.UTC(2026, 9, 13, 13, 20)
const today: Range = { kind: 'relative', unit: 'day', count: 1 }
const lastWeek: Range = { kind: 'relative', unit: 'day', count: 7 }
const weekContainingNow: ResolvedRange = { start: Date.UTC(2026, 9, 6, 22), end: tuesday.end, timezone: 'Europe/Vienna' }

const views: Metric = { t: 'count', v: 3630 }
const above: Comparison = { previous: { t: 'count', v: 3241 }, change: 389, relative_change: 0.12 }
const estimate: Projection = { t: 'estimate', value: 4900, low: 4600, high: 5200, basis_days: 8 }
const noHistory: Projection = { t: 'unavailable', reason: 'insufficient_history' }

const input = (overrides: Partial<HeadlineInput>): HeadlineInput => ({
  total: views,
  comparison: above,
  projection: estimate,
  compare: 'typical_weekday',
  range: today,
  resolved: tuesday,
  now,
  ...overrides,
})

/** The headline as a person reads it, with placeholders filled the way `Headline.vue` fills them. */
function read(sentence: Headline, locale: LocaleCode, measure: string): string {
  composer.locale.value = locale
  const number = (value: number) => new Intl.NumberFormat(locale).format(value)
  const percent = (ratio: number) => new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }).format(ratio)
  const change = sentence.change
    ? composer.t(sentence.change.key, {
        percent: sentence.change.ratio === null ? '' : percent(sentence.change.ratio),
        weekday: sentence.change.weekday ? composer.t(`explore.weekdays.${sentence.change.weekday}`) : '',
      })
    : ''
  const range = composer.t(sentence.range.key, sentence.range.named)
  const value = views.t === 'count' ? number(views.v) : ''
  const parts = [composer.t(sentence.lead, { value, measure, range, change })]
  if (sentence.pace && sentence.estimate) {
    parts.push(composer.t(sentence.pace, { projection: number(sentence.estimate.value), change }))
  }
  return [...parts, ...sentence.notes.map((note) => composer.t(note))].join(' ')
}

const en = (sentence: Headline) => read(sentence, 'en', 'views')
const de = (sentence: Headline) => read(sentence, 'de', 'Aufrufe')

afterEach(() => {
  composer.locale.value = 'en'
})

describe('headline grammar', () => {
  it('states the pace by midnight when today has an estimate', () => {
    const sentence = headline(input({}))
    expect(en(sentence)).toBe('3,630 views so far today. On pace for 4,900 by midnight, 12% above a typical Tuesday.')
    expect(de(sentence)).toBe(
      '3.630 Aufrufe bisher heute. Hochgerechnet 4.900 bis Mitternacht, 12\u00a0% mehr als an einem typischen Dienstag.'
    )
    expect(sentence.estimate).toEqual(estimate)
  })

  it('names the band and basis of the estimate', () => {
    const sentence = headline(input({}))
    const basis = (locale: LocaleCode) => {
      composer.locale.value = locale
      return sentence.basis ? composer.t(sentence.basis, { count: 8 }) : ''
    }
    expect(basis('en')).toBe('Estimate from the last 8 Tuesdays.')
    expect(basis('de')).toBe('Schätzung aus den letzten 8 Dienstagen.')
  })

  it('says why there is no estimate when history is short', () => {
    const sentence = headline(input({ projection: noHistory }))
    expect(en(sentence)).toBe('3,630 views so far today, 12% above a typical Tuesday. Not enough history for an estimate yet.')
    expect(de(sentence)).toBe(
      '3.630 Aufrufe bisher heute, 12\u00a0% mehr als an einem typischen Dienstag. Noch nicht genug Verlauf für eine Schätzung.'
    )
  })

  it('leaves the estimate out silently while the server has none for another reason', () => {
    const sentence = headline(input({ projection: { t: 'unavailable', reason: 'no_values' } }))
    expect(en(sentence)).toBe('3,630 views so far today, 12% above a typical Tuesday.')
  })

  it('compares a past range without an estimate', () => {
    const sentence = headline(
      input({
        range: { kind: 'days', start: '2026-10-01', end: '2026-10-07' },
        resolved: { start: Date.UTC(2026, 8, 30, 22), end: Date.UTC(2026, 9, 7, 22), timezone: 'Europe/Vienna' },
        compare: 'previous_period',
        comparison: { previous: { t: 'count', v: 3946 }, change: -316, relative_change: -0.08 },
      })
    )
    expect(en(sentence)).toBe('3,630 views from 2026-10-01 to 2026-10-07, 8% below the previous period.')
    expect(de(sentence)).toBe('3.630 Aufrufe vom 2026-10-01 bis 2026-10-07, 8\u00a0% weniger als im vorherigen Zeitraum.')
    expect(sentence.estimate).toBeNull()
  })

  it('reads plainly without a comparison', () => {
    const sentence = headline(input({ compare: 'none', comparison: undefined, projection: undefined }))
    expect(en(sentence)).toBe('3,630 views today.')
    expect(de(sentence)).toBe('3.630 Aufrufe heute.')
  })

  it('reads within ±5 % as about the same', () => {
    const sentence = headline(
      input({ projection: undefined, comparison: { previous: { t: 'count', v: 3457 }, change: 173, relative_change: 0.05 } })
    )
    expect(en(sentence)).toBe('3,630 views so far today, about the same as a typical Tuesday.')
    expect(de(sentence)).toBe('3.630 Aufrufe bisher heute, etwa so viel wie an einem typischen Dienstag.')
  })

  it('reads a zero baseline as new', () => {
    const sentence = headline(
      input({ projection: undefined, comparison: { previous: { t: 'count', v: 0 }, change: 3630, reason: 'zero_baseline' } })
    )
    expect(en(sentence)).toBe('3,630 views so far today, new, none on a typical Tuesday.')
    expect(de(sentence)).toBe('3.630 Aufrufe bisher heute, neu, an einem typischen Dienstag keine.')
  })

  it('projects a multi-day range containing now to the end of today', () => {
    const sentence = headline(
      input({ range: lastWeek, resolved: weekContainingNow, comparison: { ...above, relative_change: 0.2 } })
    )
    expect(en(sentence)).toBe(
      '3,630 views so far in the last 7 days. On pace for 4,900 by the end of today, 20% above typical weekdays.'
    )
    expect(de(sentence)).toBe(
      '3.630 Aufrufe bisher in den letzten 7 Tagen. Hochgerechnet 4.900 bis Ende des heutigen Tages, 20\u00a0% mehr als an typischen Wochentagen.'
    )
  })

  it('reads as uncompared when a typical weekday lacks history', () => {
    const sentence = headline(
      input({
        projection: noHistory,
        comparison: { previous: { t: 'unavailable', reason: 'insufficient_history' }, reason: 'insufficient_history' },
      })
    )
    expect(en(sentence)).toBe('3,630 views today. Not enough history for a typical day yet.')
    expect(de(sentence)).toBe('3.630 Aufrufe heute. Noch nicht genug Verlauf für einen typischen Tag.')
  })
})

/** A referrer mover from 09:40 local time, computed at 15:20. */
const mover = (sofar: number, typical: number, share: number): Finding => ({
  rank: 1,
  rule: 'dimension_mover',
  rule_version: 1,
  computed_at: now,
  window_start: Date.UTC(2026, 9, 13, 7, 40),
  share,
  state: {
    v: 1,
    site: 'site1',
    measures: ['visitor_days'],
    dimensions: [],
    filter: {
      kind: 'group',
      op: 'and',
      items: [{ kind: 'cond', id: 'f1', field: 'referrer_domain', cmp: 'eq', value: { t: 'string', v: 'instagram.com' } }],
    },
    range: today,
    timezone: 'Europe/Vienna',
    granularity: 'hour',
    compare: 'typical_weekday',
    metric_version: 1,
    sort: [],
    page: { size: 25, index: 0 },
    visualization: 'line',
  },
  values: {
    kind: 'dimension_mover',
    dimension: 'referrer_domain',
    value: { t: 'string', v: 'instagram.com' },
    today: sofar,
    typical,
    ratio: sofar / typical,
    site_today: 900,
    site_typical: 600,
  },
})
const lift = mover(312, 98, 0.64)
const party: Annotation = { id: 'a1', site: 'site1', at: Date.UTC(2026, 9, 13, 8), text: '33rd birthday party', created_at: now }

const whyInput = (overrides: Partial<WhyInput>): WhyInput => ({
  findings: [lift],
  stale: false,
  annotations: [],
  compared: true,
  resolved: tuesday,
  now,
  ...overrides,
})

/** The line as `Headline.vue` fills it. */
function readWhy(line: Why | null, locale: LocaleCode): string | null {
  if (!line) return null
  composer.locale.value = locale
  if (line.kind === 'usual') return composer.t(line.key)
  return composer.t(line.key, {
    value: line.mover.value.t === 'string' ? line.mover.value.v : '',
    change: new Intl.NumberFormat(locale, { signDisplay: 'exceptZero' }).format(line.change),
    time: new Intl.DateTimeFormat(locale, { timeZone: tuesday.timezone, timeStyle: 'short' }).format(line.since),
    annotation: line.annotation?.text ?? '',
  })
}

describe('why line', () => {
  it('names the mover behind most of a lift', () => {
    const line = why(whyInput({}))
    expect(readWhy(line, 'en')).toBe('Most of the lift is instagram.com: +214 visitor-days since 9:40 AM.')
    expect(readWhy(line, 'de')).toBe('Der Großteil des Anstiegs entfällt auf instagram.com: +214 Besuchertage seit 09:40.')
    expect(line?.kind === 'mover' && line.filter).toEqual(lift.state.filter)
  })

  it('names the mover behind most of a drop', () => {
    const line = why(whyInput({ findings: [mover(40, 120, 0.7)] }))
    expect(readWhy(line, 'en')).toBe('Most of the drop is instagram.com: -80 visitor-days since 9:40 AM.')
    expect(readWhy(line, 'de')).toBe('Der Großteil des Rückgangs entfällt auf instagram.com: -80 Besuchertage seit 09:40.')
  })

  it('reads as usual when no mover explains half the change', () => {
    const line = why(whyInput({ findings: [mover(312, 98, 0.49)] }))
    expect(readWhy(line, 'en')).toBe('Everything is within its usual range.')
    expect(readWhy(line, 'de')).toBe('Alles liegt im üblichen Bereich.')
  })

  it('names an annotation inside the window', () => {
    const line = why(whyInput({ annotations: [party] }))
    expect(readWhy(line, 'en')).toBe(
      'Most of the lift is instagram.com: +214 visitor-days since 9:40 AM, after the annotation "33rd birthday party".'
    )
    expect(readWhy(line, 'de')).toBe(
      'Der Großteil des Anstiegs entfällt auf instagram.com: +214 Besuchertage seit 09:40, nach der Notiz "33rd birthday party".'
    )
  })

  it('leaves out an annotation before the window', () => {
    const line = why(whyInput({ annotations: [{ ...party, at: Date.UTC(2026, 9, 13, 6) }] }))
    expect(readWhy(line, 'en')).toBe('Most of the lift is instagram.com: +214 visitor-days since 9:40 AM.')
    expect(readWhy(line, 'de')).toBe('Der Großteil des Anstiegs entfällt auf instagram.com: +214 Besuchertage seit 09:40.')
  })

  it('says nothing from stale findings', () => {
    expect(why(whyInput({ stale: true }))).toBeNull()
  })

  it('says nothing without a comparison', () => {
    expect(why(whyInput({ compared: false }))).toBeNull()
    expect(why(whyInput({ compared: false, findings: [] }))).toBeNull()
  })
})
