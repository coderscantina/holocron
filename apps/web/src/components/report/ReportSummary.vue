<script setup lang="ts">
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Field, ReportResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * A report result in one line and its headline number, for board cards. A split funnel adds
 * "split by {field}" and its top three groups by visitors entered.
 */
const props = defineProps<{ report: ReportResult; splitBy?: Field }>()

const { t } = useI18n()
const format = useExplorationFormat()
const count = (v: number) => format.metric({ t: 'count', v }, true)

const GROUPS_ON_CARD = 3
const groups = computed(() => {
  const report = props.report
  if (report.kind !== 'funnel' || !props.splitBy) return []
  const field = props.splitBy
  return report.groups.slice(0, GROUPS_ON_CARD).map((group) => ({
    label: format.cellLabel(field, group.value),
    entered: count(group.steps[0]?.reached ?? 0),
    conversion: format.metric(group.conversion),
  }))
})

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
    <template v-if="splitBy && report.kind === 'funnel'">
      <p class="mt-1 text-xs text-muted">{{ t('reports.funnel.split.splitBy', { field: format.fieldLabel(splitBy) }) }}</p>
      <ul class="text-xs">
        <li
          v-for="group in groups"
          :key="group.label"
          class="flex items-baseline gap-2 border-t border-divider py-1"
        >
          <span class="min-w-0 flex-1 truncate text-foreground">{{ group.label }}</span>
          <span class="text-muted tabular-nums">{{ group.entered }}</span>
          <span class="w-12 text-right font-medium text-primary tabular-nums">{{ group.conversion }}</span>
        </li>
      </ul>
    </template>
  </div>
</template>
