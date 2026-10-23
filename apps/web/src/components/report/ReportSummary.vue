<script setup lang="ts">
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { ReportResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** A report result in one line and its headline number, for board cards. */
const props = defineProps<{ report: ReportResult }>()

const { t } = useI18n()
const format = useExplorationFormat()
const count = (v: number) => format.metric({ t: 'count', v }, true)

const summary = computed<{ headline: string; text: string } | null>(() => {
  const report = props.report
  switch (report.kind) {
    case 'funnel':
      return {
        headline: format.metric(report.conversion),
        text: t('reports.summary.funnel', {
          last: count(report.steps.at(-1)?.reached ?? 0),
          first: count(report.steps[0]?.reached ?? 0),
        }),
      }
    case 'journey':
      return {
        headline: count(report.entry_sessions),
        text: t('reports.summary.journey', { sessions: count(report.entry_sessions), distinct: count(report.distinct_sequences) }),
      }
    case 'attribution': {
      const total = report.conversions.reduce(
        (sum, c) => sum + c.direct.conversions + c.touches.reduce((s, row) => s + row.attributed.conversions, 0),
        0
      )
      const direct = report.conversions.reduce((sum, c) => sum + c.direct.conversions, 0)
      return { headline: count(total), text: t('reports.summary.attribution', { conversions: count(total), direct: count(direct) }) }
    }
    case 'goals':
      return {
        headline: report.goals[0] ? format.metric(report.goals[0].rate) : '–',
        text: t('reports.summary.goals', { count: report.goals.length }),
      }
    case 'session_list':
      return { headline: count(report.row_count), text: t('reports.summary.sessions', { count: count(report.row_count) }) }
    case 'retention':
      return null
  }
})
</script>

<template>
  <div
    v-if="summary"
    class="grid gap-0.5"
  >
    <p class="text-lg font-semibold text-primary tabular-nums">{{ summary.headline }}</p>
    <p class="text-xs text-muted">{{ summary.text }}</p>
  </div>
</template>
