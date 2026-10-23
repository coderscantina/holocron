import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { ExploreResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * What a result covers and can claim, as the server reported it, in words: the resolved local
 * dates, timezone, correction state and metric version (the routine facts), and the
 * retention clip (the exception worth showing on the page).
 */
export function useFreshnessFacts(result: MaybeRefOrGetter<ExploreResult | null>) {
  const { t } = useI18n()
  const format = useExplorationFormat()

  return computed(() => {
    const current = toValue(result)
    if (!current) return null
    const { start, end, timezone, retention_start } = current.resolved
    const pending = current.freshness.correction === 'pending'
    return {
      range: t('explore.freshness.range', {
        start: format.date(start, timezone),
        end: format.date(Math.max(start, end - 1), timezone),
      }),
      timezone,
      pending,
      correction: pending
        ? t('explore.freshness.pending', { count: current.freshness.dirty_days })
        : t('explore.freshness.corrected'),
      version: t('explore.freshness.version', { version: current.freshness.metric_version }),
      retention: retention_start ? t('explore.freshness.retention') : null,
    }
  })
}
