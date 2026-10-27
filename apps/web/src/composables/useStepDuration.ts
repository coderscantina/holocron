import { durationParts, millisOf } from '~/lib/funnel-duration'
import type { Metric } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import { useFormat } from './useFormat'

/** Step durations in words the locale's catalog gives: "under 1 s", "2 min 39 s", "3 d 4 h". */
export function useStepDuration() {
  const { t } = useI18n()
  const numbers = useFormat()

  const span = (millis: number): string => {
    const parts = durationParts(millis)
    if (parts.length === 0) return t('reports.funnel.duration.under')
    return parts.map(({ unit, value }) => t(`reports.funnel.duration.${unit}`, { value: numbers.integer(value) })).join(' ')
  }

  /** `null` when nobody reached the step, so the caller shows no duration at all. */
  const metric = (value: Metric): string | null => {
    const millis = millisOf(value)
    return millis === null ? null : span(millis)
  }

  return { span, metric }
}
