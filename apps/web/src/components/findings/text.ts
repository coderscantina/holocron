import type { Cell, Field, Finding, GoalId, Metric } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** What the templates need to turn stored values into words in the active locale. */
export interface FindingFormat {
  t: (key: MessageKey, named?: Record<string, unknown>) => string
  integer: (value: number) => string
  percent: (ratio: number) => string
  /** The time of day of an instant in a timezone. */
  clock: (ms: number, timezone: string) => string
  /** A local date `YYYY-MM-DD` as a date. */
  day: (date: string) => string
  field: (field: Field) => string
  cell: (field: Field, cell: Cell) => string
  vital: (metric: Metric) => string
  /** The name of a goal, if the site still has it. */
  goal: (goal: GoalId) => string | undefined
}

export interface FindingText {
  /** Kind and scope in faint text, "Funnel · Ticket checkout". */
  kind: string
  /** The fact, one sentence. */
  fact: string
  /** The measured detail, one or two sentences. */
  detail: string
  /** False for a rule or version this build has no template for. */
  known: boolean
}

/** Rule versions with a template here; a finding of any other version renders generically. */
const KNOWN_VERSION = 1

/** A change against the typical value as one of the rise, fall and new-from-zero sentences. */
function change(
  f: FindingFormat,
  keys: { rise: MessageKey; fall: MessageKey; new: MessageKey },
  today: number,
  typical: number,
  named: Record<string, unknown>
): string {
  const shared = { ...named, today: f.integer(today) }
  if (typical <= 0) return f.t(keys.new, shared)
  const ratio = (today - typical) / typical
  return f.t(ratio >= 0 ? keys.rise : keys.fall, { ...shared, change: f.percent(Math.abs(ratio)) })
}

/**
 * Words for a finding from its stored values: fixed templates per rule and version, facts
 * only, never a cause. A rule or version without a template, or values that do not belong to
 * the rule, give the generic sentence, so a newer server never breaks the card.
 */
export function findingText(finding: Finding, f: FindingFormat): FindingText {
  const { values } = finding
  if (finding.rule_version === KNOWN_VERSION && finding.rule === values.kind) {
    switch (values.kind) {
      case 'dimension_mover': {
        const value = f.cell(values.dimension, values.value)
        return {
          known: true,
          kind: f.t('findings.kind.mover', { field: f.field(values.dimension) }),
          fact: change(
            f,
            { rise: 'findings.mover.rise', fall: 'findings.mover.fall', new: 'findings.mover.new' },
            values.today,
            values.typical,
            { value }
          ),
          detail: f.t('findings.mover.detail', {
            typical: f.integer(values.typical),
            siteToday: f.integer(values.site_today),
            siteTypical: f.integer(values.site_typical),
          }),
        }
      }
      case 'goal_pace':
        return {
          known: true,
          kind: f.t('findings.kind.goal'),
          fact: change(
            f,
            { rise: 'findings.goal.rise', fall: 'findings.goal.fall', new: 'findings.goal.new' },
            values.today,
            values.typical,
            { goal: f.goal(values.goal) ?? f.t('findings.goal.unnamed') }
          ),
          detail: f.t('findings.goal.detail', { typical: f.integer(values.typical) }),
        }
      case 'funnel_step':
        return {
          known: true,
          kind: f.t('findings.kind.funnel', { title: values.title }),
          fact: f.t('findings.funnel.fact', {
            step: values.step,
            dropOff: f.percent(values.drop_off),
            baseline: f.percent(values.baseline_drop_off),
          }),
          detail: f.t('findings.funnel.detail', {
            dropped: f.integer(values.dropped),
            entered: f.integer(values.entered),
            hour: `${String(values.until_hour).padStart(2, '0')}:00`,
            baselineDropped: f.integer(values.baseline_dropped),
            baselineEntered: f.integer(values.baseline_entered),
            days: values.baseline_days,
          }),
        }
      case 'performance':
        return {
          known: true,
          kind: f.t('findings.kind.performance'),
          fact: f.t('findings.performance.fact', {
            metric: values.metric.toUpperCase(),
            value: f.vital(values.value),
            device: f.cell('device', values.device),
            days: values.days,
          }),
          detail: f.t('findings.performance.detail', {
            slowest: f.vital(values.slowest),
            day: f.day(values.slowest_day),
            observations: f.integer(values.observations),
          }),
        }
    }
  }
  return {
    known: false,
    kind: f.t('findings.kind.generic'),
    fact: f.t('findings.generic.fact', { field: genericField(finding, f) }),
    detail: f.t('findings.generic.detail'),
  }
}

/** The first field the finding's state filters on, else the site as a whole. */
function genericField(finding: Finding, f: FindingFormat): string {
  const first = finding.state.filter?.items.find((node) => node.kind === 'cond')
  return first ? f.field(first.field) : f.t('findings.generic.field')
}
