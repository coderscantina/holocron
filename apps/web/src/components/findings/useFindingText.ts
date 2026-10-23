import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useGoals } from '~/composables/useGoals'
import { useVitalFormat } from '~/composables/useVitalFormat'
import type { Finding, Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import { findingText, type FindingFormat } from './text'

/** Words for findings in the active locale; goals are named from the site's own goal list. */
export function useFindingText(site: MaybeRefOrGetter<Pick<Site, 'id' | 'timezone'>>) {
  const { t, locale } = useI18n()
  const format = useFormat()
  const exploration = useExplorationFormat()
  const vital = useVitalFormat()
  const { goals } = useGoals(() => toValue(site).id)

  const formats = computed<FindingFormat>(() => {
    const names = new Map(goals.value.map((goal) => [goal.id, goal.name]))
    return {
      t,
      integer: format.integer,
      percent: format.percent,
      clock: (ms, timezone) =>
        new Intl.DateTimeFormat(locale.value, { timeZone: timezone, timeStyle: 'short' }).format(ms),
      day: (date) =>
        new Intl.DateTimeFormat(locale.value, { timeZone: 'UTC', dateStyle: 'medium' }).format(Date.parse(date)),
      field: exploration.fieldLabel,
      cell: exploration.cellLabel,
      vital,
      goal: (goal) => names.get(goal),
    }
  })

  return {
    text: (finding: Finding) => findingText(finding, formats.value),
    /** The time of day of an instant in the site's timezone. */
    clock: (ms: number) => formats.value.clock(ms, toValue(site).timezone),
  }
}
