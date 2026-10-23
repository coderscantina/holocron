<script setup lang="ts">
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useVitalFormat } from '~/composables/useVitalFormat'
import CountryFlag from '~/components/CountryFlag.vue'
import type { Cut, Field } from '~/generated'
import { countryCode } from '~/lib/client-looks'
import { isVital, rating, vitalLabels, type Rating } from '~/lib/vitals'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * Nearest-rank p50, p75 and p95 per metric and, when the view is broken down, per group
 * (0003 Percentiles). Ratings are words next to the value, never colour alone.
 */
const props = defineProps<{
  cut: Cut
  /** The cut's dimensions: `perf_metric`, then the optional breakdown. */
  dimensions: Field[]
}>()

const { t } = useI18n()
const format = useExplorationFormat()
const percentile = useVitalFormat()
const regionId = useId()

const group = computed(() => props.dimensions[1] ?? null)
const ratingLabels = {
  good: 'performance.rating.good',
  improve: 'performance.rating.improve',
  poor: 'performance.rating.poor',
} as const satisfies Record<Rating, MessageKey>
const tone: Record<Rating, string> = { good: 'text-success', improve: 'text-warning', poor: 'text-destructive' }

const rows = computed(() =>
  props.cut.rows.flatMap((row) => {
    const metric = row.key[0]
    if (metric?.t !== 'string' || !isVital(metric.v)) return []
    const vital = metric.v
    const field = group.value
    const cell = row.key[1]
    return [
      {
        vital,
        group: field && cell ? format.cellLabel(field, cell) : null,
        flag: field && cell ? countryCode(field, cell) : null,
        values: row.values,
        rating: rating(vital, row.values[1]),
      },
    ]
  })
)
</script>

<template>
  <div class="overflow-hidden rounded-panel border border-hairline bg-card">
    <div
      :id="regionId"
      role="region"
      tabindex="0"
      :aria-label="t('performance.table.label')"
      class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
    >
      <table class="w-full text-sm tabular-nums [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
        <caption class="sr-only">{{ t('performance.table.caption') }}</caption>
        <thead class="bg-fill text-[0.875rem] text-muted">
          <tr>
            <th
              scope="col"
              class="h-table-head px-4 text-left font-medium"
            >
              {{ format.fieldLabel('perf_metric') }}
            </th>
            <th
              v-if="group"
              scope="col"
              class="h-table-head px-4 text-left font-medium"
            >
              {{ format.fieldLabel(group) }}
            </th>
            <th
              v-for="measure in ['p50', 'p75', 'p95'] as const"
              :key="measure"
              scope="col"
              class="h-table-head px-4 text-right font-medium"
            >
              {{ format.measureLabel(measure) }}
            </th>
            <th
              scope="col"
              class="h-table-head px-4 text-left font-medium"
            >
              {{ t('performance.table.rating') }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(row, index) in rows"
            :key="index"
            class="h-table-row border-t border-divider"
          >
            <th
              scope="row"
              class="px-4 py-1 text-left font-medium whitespace-nowrap text-primary"
            >
              {{ t(vitalLabels[row.vital]) }}
            </th>
            <td
              v-if="group"
              class="max-w-64 px-4 py-1"
            >
              <span class="flex min-w-0 items-center gap-2">
                <CountryFlag
                  v-if="row.flag"
                  :code="row.flag"
                />
                <span class="truncate">{{ row.group }}</span>
              </span>
            </td>
            <td
              v-for="(value, column) in row.values"
              :key="column"
              class="px-4 py-1 text-right whitespace-nowrap"
              :class="column === 1 ? 'font-semibold text-primary' : ''"
            >
              {{ percentile(value) }}
            </td>
            <td
              class="px-4 py-1 whitespace-nowrap"
              :class="row.rating ? tone[row.rating] : 'text-muted'"
            >
              {{ row.rating ? t(ratingLabels[row.rating]) : '–' }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
