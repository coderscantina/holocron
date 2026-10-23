<script setup lang="ts">
import type { Granularity, Metric, RetentionResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Cohorts as rows and calendar offsets as columns (0003 Retention and cohorts). The table is the
 * visualization: each cell is shaded by its rate, and the same values are in the cells as text,
 * so screen readers and the eye read the same table. Incomplete intervals are dashed and named.
 */
const props = defineProps<{
  result: RetentionResult
  granularity: Granularity
  timezone: string
}>()

const { t, locale } = useI18n()
const regionId = useId()

const offsets = computed(() => props.result.averages.map((_, index) => index))
const unit = computed(() => (props.granularity === 'week' ? 'retention.table.week' : 'retention.table.day'))

const percent = (metric: Metric | undefined) =>
  metric?.t === 'ratio'
    ? new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 1 }).format(metric.v)
    : '–'
const shade = (metric: Metric | undefined) => (metric?.t === 'ratio' ? 0.06 + 0.5 * metric.v : 0)
const cohortLabel = (local: string) =>
  new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeZone: 'UTC' }).format(
    Date.parse(`${local}T00:00:00Z`)
  )
const integer = (value: number) => new Intl.NumberFormat(locale.value).format(value)
</script>

<template>
  <div class="overflow-hidden rounded-panel border border-hairline bg-card">
    <div
      :id="regionId"
      role="region"
      tabindex="0"
      :aria-label="t('retention.table.label')"
      class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
    >
      <table class="w-full text-sm tabular-nums [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
        <caption class="sr-only">{{ t('retention.table.caption', { timezone }) }}</caption>
        <thead>
          <tr class="h-table-head bg-fill text-left text-[0.875rem] text-muted">
            <th
              scope="col"
              class="sticky left-0 bg-fill px-4 font-medium"
            >
              {{ t('retention.table.cohort') }}
            </th>
            <th
              scope="col"
              class="px-4 text-right font-medium"
            >
              {{ t('retention.table.size') }}
            </th>
            <th
              v-for="offset in offsets"
              :key="offset"
              scope="col"
              class="min-w-16 px-1.5 text-center font-medium"
            >
              {{ t(unit, { offset }) }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="cohort in result.cohorts"
            :key="cohort.start"
            class="border-t border-divider"
          >
            <th
              scope="row"
              class="sticky left-0 bg-card px-4 py-1.5 text-left font-medium whitespace-nowrap text-primary"
            >
              {{ cohortLabel(cohort.local) }}
            </th>
            <td class="px-4 py-1.5 text-right">{{ integer(cohort.size) }}</td>
            <td
              v-for="(cell, offset) in cohort.cells"
              :key="offset"
              class="p-0.5"
            >
              <div
                class="relative rounded-sm px-1.5 py-1 text-center"
                :class="cell.complete ? '' : 'border border-dashed border-border-strong text-muted'"
                :title="t('retention.table.cellTitle', { active: integer(cell.active), size: integer(cohort.size) })"
              >
                <span
                  class="absolute inset-0 rounded-sm bg-series-primary"
                  :style="{ opacity: shade(cell.rate) }"
                  aria-hidden="true"
                />
                <span class="relative block font-semibold">{{ percent(cell.rate) }}</span>
                <span class="relative block text-2xs text-muted">{{ integer(cell.active) }}</span>
                <span
                  v-if="!cell.complete"
                  class="sr-only"
                >
                  {{ t('retention.table.incomplete') }}
                </span>
              </div>
            </td>
          </tr>
        </tbody>
        <tfoot>
          <tr class="h-table-row border-t border-divider bg-fill">
            <th
              scope="row"
              class="sticky left-0 bg-fill px-4 text-left font-semibold text-primary"
            >
              {{ t('retention.table.average') }}
            </th>
            <td class="px-4" />
            <td
              v-for="(average, offset) in result.averages"
              :key="offset"
              class="px-1.5 text-center font-semibold text-primary"
            >
              {{ percent(average) }}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  </div>
</template>
