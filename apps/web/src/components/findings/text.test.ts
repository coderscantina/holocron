import { afterEach, describe, expect, it } from 'vitest'

import type { Finding, FindingValues } from '~/generated'
import { findingBadges } from '~/composables/useNavigation'
import { composer, useI18n } from '~/plugins/i18n'
import { overviewState } from '~/services/exploration'

import { findingActions, filterOf, listState, openFinding, type ActionContext } from './actions'
import { findingText, type FindingFormat } from './text'

const format: FindingFormat = {
  t: useI18n().t,
  integer: (n) => String(n),
  percent: (ratio) => `${Math.round(ratio * 100)} %`,
  clock: () => '14:05',
  day: (date) => date,
  field: (field) => (typeof field === 'string' ? field : field.key),
  cell: (_, cell) => ('v' in cell ? String(cell.v) : cell.t),
  vital: (metric) => (metric.t === 'millis' ? `${metric.v} ms` : metric.t),
  goal: (goal) => (goal === 'g1' ? 'Newsletter signup' : undefined),
}

const finding = (values: FindingValues, over: Partial<Finding> = {}): Finding => ({
  rank: 1,
  rule: values.kind,
  rule_version: 1,
  computed_at: 0,
  window_start: 0,
  share: 0,
  state: { ...overviewState('site', 'Europe/Vienna'), filter: { kind: 'group', op: 'and', items: [] } },
  values,
  ...over,
})

const mover = finding({
  kind: 'dimension_mover',
  dimension: 'referrer_domain',
  value: { t: 'string', v: 'instagram.com' },
  today: 312,
  typical: 98,
  ratio: 3.18,
  site_today: 900,
  site_typical: 600,
})
const goal = finding({ kind: 'goal_pace', goal: 'g1', goal_version: 1, today: 5, typical: 10, relative_change: -0.5 })
const funnel = finding({
  kind: 'funnel_step',
  card: 'c1',
  title: 'Ticket checkout',
  step: 2,
  until_hour: 14,
  entered: 214,
  dropped: 86,
  baseline_entered: 1000,
  baseline_dropped: 310,
  baseline_days: 8,
  drop_off: 0.4,
  baseline_drop_off: 0.31,
})
const performance = finding({
  kind: 'performance',
  metric: 'lcp',
  device: { t: 'string', v: 'mobile' },
  value: { t: 'millis', v: 3400 },
  observations: 1420,
  slowest: { t: 'millis', v: 2600 },
  slowest_day: '2026-10-01',
  days: 28,
})

afterEach(() => {
  composer.locale.value = 'en'
})

describe('findingText', () => {
  it('states each family from its stored values in English', () => {
    expect(findingText(mover, format)).toMatchObject({
      known: true,
      kind: 'referrer_domain · today',
      fact: 'instagram.com is up 218 % on a typical weekday, 312 visitor-days so far',
      detail: 'Typical by now: 98. All traffic: 900 against 600.',
    })
    expect(findingText(goal, format).fact).toBe('Newsletter signup is down 50 % on a typical weekday, 5 completions so far')
    expect(findingText(funnel, format)).toMatchObject({
      kind: 'Funnel · Ticket checkout',
      fact: 'Drop-off after step 2 is 40 % today, 31 % on typical days',
      detail: '86 of 214 visitors until 14:00, against 310 of 1000 over 8 days.',
    })
    expect(findingText(performance, format)).toMatchObject({
      fact: 'LCP p75 is 3400 ms on mobile, the slowest in 28 days',
      detail: 'The slowest day before was 2600 ms on 2026-10-01. 1420 observations in the last 24 hours.',
    })
  })

  it('states the same facts in German', () => {
    composer.locale.value = 'de'
    expect(findingText(mover, format).fact).toBe(
      'instagram.com liegt 218 % über einem typischen Wochentag, bisher 312 Besuchertage'
    )
    expect(findingText(funnel, format).fact).toBe(
      'Die Abbruchquote nach Schritt 2 beträgt heute 40 %, an typischen Tagen 31 %'
    )
  })

  it('says there was nothing to compare with instead of a percentage', () => {
    const fresh = finding({ ...(goal.values as Extract<FindingValues, { kind: 'goal_pace' }>), typical: 0, today: 7 })
    expect(findingText(fresh, format).fact).toBe('Newsletter signup has 7 completions so far, none on a typical weekday by now')
  })

  it('falls back to a generic sentence for an unknown rule or version, in both languages', () => {
    const unknownRule = { ...mover, rule: 'seasonality' as Finding['rule'] }
    const newer = { ...mover, rule_version: 2 }
    for (const unknown of [unknownRule, newer]) {
      composer.locale.value = 'en'
      expect(findingText(unknown, format)).toMatchObject({
        known: false,
        fact: 'Something changed in this site',
      })
      composer.locale.value = 'de'
      expect(findingText(unknown, format).fact).toBe('In dieser Website hat sich etwas geändert')
    }
  })

  it('names the field the state filters on when it has one', () => {
    const filtered = finding(mover.values, {
      rule_version: 9,
      state: {
        ...mover.state,
        filter: { kind: 'group', op: 'and', items: [{ kind: 'cond', id: 'f1', field: 'referrer_domain', cmp: 'eq', value: { t: 'string', v: 'x' } }] },
      },
    })
    expect(findingText(filtered, format).fact).toBe('Something changed in referrer_domain')
  })
})

describe('findingActions', () => {
  const ids = (actions: ReturnType<typeof findingActions>) => actions.map((action) => action.id)
  const context: ActionContext = { replays: 12, canAnnotate: true }

  it('offers filter, replays and annotation for a mover, filtering first', () => {
    expect(ids(findingActions(mover, true, context))).toEqual(['filter', 'replays', 'annotate'])
  })

  it('leaves out the replay action unless recording gave a count above 0', () => {
    expect(ids(findingActions(mover, true, { ...context, replays: null }))).toEqual(['filter', 'annotate'])
    expect(ids(findingActions(mover, true, { ...context, replays: 0 }))).toEqual(['filter', 'annotate'])
  })

  it('opens the report of the family and leaves out annotation without write access', () => {
    const read = { replays: null, canAnnotate: false }
    expect(findingActions(goal, true, read)).toEqual([{ id: 'report', route: 'goals' }])
    expect(findingActions(funnel, true, read)).toEqual([{ id: 'report', route: 'funnels' }])
    expect(findingActions(performance, true, read)).toEqual([{ id: 'report', route: 'performance' }])
  })

  it('only links an unknown rule to its state', () => {
    expect(ids(findingActions(mover, false, context))).toEqual(['details', 'annotate'])
    expect(filterOf(mover, false)).toBeNull()
  })
})

describe('listState', () => {
  const now = Date.UTC(2026, 9, 15, 12)
  it('waits for the first summary, then shows findings until the summary is 30 minutes old', () => {
    expect(listState(null, now)).toBe('pending')
    expect(listState(now - 29 * 60_000, now)).toBe('current')
    expect(listState(now - 31 * 60_000, now)).toBe('behind')
  })
})

describe('openFinding', () => {
  it('opens the report of the family with the finding state, and Explore for the rest', () => {
    const name = (f: Finding, known = true) => openFinding('site', f, known)
    expect(name(goal)).toMatchObject({ name: 'goals', params: { siteId: 'site' }, query: { s: expect.any(String) } })
    expect(name(funnel)).toMatchObject({ name: 'funnels' })
    expect(name(performance)).toMatchObject({ name: 'performance' })
    expect(name(mover)).toMatchObject({ name: 'explore' })
    expect(name(goal, false)).toMatchObject({ name: 'explore' })
  })
})

describe('findingBadges', () => {
  it('counts the findings of each family on its navigation item and skips movers', () => {
    expect(findingBadges([mover, goal, goal, funnel, performance, mover])).toEqual(
      new Map([
        ['goals', 2],
        ['funnels', 1],
        ['performance', 1],
      ])
    )
    expect(findingBadges([mover])).toEqual(new Map())
  })
})
