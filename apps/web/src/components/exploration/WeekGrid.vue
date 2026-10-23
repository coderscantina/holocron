<script setup lang="ts">
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Measure, Row } from '~/generated'
import { weekdayLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'

/**
 * Weekday by hour of the first measure (0004 Weekday and hour), with the same values as a table
 * for assistive technology. Shade carries magnitude; every cell also has its number as a title
 * and in the table.
 */
const props = defineProps<{
  rows: Row[]
  measure: Measure
  label: string
}>()

const { t } = useI18n()
const format = useExplorationFormat()

const days = [1, 2, 3, 4, 5, 6, 7] as const
const hours = Array.from({ length: 24 }, (_, hour) => hour)

const values = computed(() => {
  const map = new Map<string, { value: number; text: string }>()
  for (const row of props.rows) {
    const [day, hour] = row.key
    const metric = row.values[0]
    if (day?.t !== 'integer' || hour?.t !== 'integer' || !metric) continue
    map.set(`${day.v}:${hour.v}`, { value: metric.t === 'count' ? metric.v : 0, text: format.metric(metric) })
  }
  return map
})
const max = computed(() => Math.max(1, ...[...values.value.values()].map((v) => v.value)))
const cell = (day: number, hour: number) => values.value.get(`${day}:${hour}`) ?? { value: 0, text: '0' }
</script>

<template>
  <figure class="m-0">
    <figcaption class="sr-only">{{ t('explore.grid.summary', { measure: label }) }}</figcaption>
    <div
      class="overflow-x-auto"
      aria-hidden="true"
    >
      <div class="grid min-w-[36rem] grid-cols-[5rem_repeat(24,minmax(0,1fr))] gap-0.5 text-[0.8125rem] text-muted">
        <span />
        <span
          v-for="hour in hours"
          :key="hour"
          class="text-center tabular-nums"
        >
          {{ hour % 3 === 0 ? String(hour).padStart(2, '0') : '' }}
        </span>
        <template
          v-for="day in days"
          :key="day"
        >
          <span class="truncate pr-1 text-right">{{ t(weekdayLabels[day]) }}</span>
          <span
            v-for="hour in hours"
            :key="hour"
            class="aspect-square rounded-xs bg-series-primary"
            :style="{ opacity: 0.08 + 0.92 * (cell(day, hour).value / max) }"
            :title="`${t(weekdayLabels[day])} ${String(hour).padStart(2, '0')}:00 · ${cell(day, hour).text}`"
          />
        </template>
      </div>
    </div>
    <div class="sr-only">
      <table>
        <caption>{{ t('explore.grid.label') }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('explore.fields.weekday') }}</th>
            <th
              v-for="hour in hours"
              :key="hour"
              scope="col"
            >
              {{ String(hour).padStart(2, '0') }}:00
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="day in days"
            :key="day"
          >
            <th scope="row">{{ t(weekdayLabels[day]) }}</th>
            <td
              v-for="hour in hours"
              :key="hour"
            >
              {{ cell(day, hour).text }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
