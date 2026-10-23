<script setup lang="ts">
import CohortHeatmap from '~/components/retention/CohortHeatmap.vue'
import StageView from '~/components/stage/StageView.vue'
import { useFormat } from '~/composables/useFormat'
import type { ExplorationState, RetentionResult, Site } from '~/generated'
import { signedPercent } from '~/lib/site-pace'
import { curvePoints, heatmap, pointList, ratio, trend, type GridPosition } from '~/lib/retention-heatmap'
import { useI18n } from '~/plugins/i18n'
import { encodeState, localDate } from '~/services/exploration'
import { sessionListState } from '~/services/reports'

/**
 * The retention heatmap: the cohort grid as the subject, and beside it the share that comes back
 * one period after the first visit, the average curve with the pointed or selected cohort drawn
 * over it, and one sentence on how newer cohorts compare. A selected cell links to the sessions
 * of its period.
 */
const props = defineProps<{
  site: Site
  state: ExplorationState
  result: RetentionResult
  timezone: string
}>()

const { t, locale } = useI18n()
const format = useFormat()

const map = computed(() => heatmap(props.result))
const week = computed(() => props.state.granularity === 'week')
const unit = computed(() => (week.value ? 'retention.table.week' : 'retention.table.day'))

const pointed = ref<number | null>(null)
const selected = ref<GridPosition | null>(null)
watch(map, () => {
  selected.value = null
  pointed.value = null
})
const activeRow = computed(() => pointed.value ?? selected.value?.row ?? null)
const active = computed(() => (activeRow.value === null ? null : (map.value.rows[activeRow.value] ?? null)))
const selectedRow = computed(() => (selected.value ? (map.value.rows[selected.value.row] ?? null) : null))
const selectedCell = computed(() => (selected.value ? (selectedRow.value?.cells[selected.value.column] ?? null) : null))

const headline = computed(() => ratio(props.result.averages[1]))
const change = computed(() => trend(props.result, 1))
const STEADY = 0.05

const dateLabel = (local: string, withYear = false) =>
  new Intl.DateTimeFormat(locale.value, { month: 'long', day: 'numeric', ...(withYear ? { year: 'numeric' } : {}), timeZone: 'UTC' }).format(
    Date.parse(`${local}T00:00:00Z`)
  )

const takeaway = computed(() => {
  const found = change.value
  if (!found) return null
  const later = t(week.value ? 'retention.heatmap.laterWeek' : 'retention.heatmap.laterDay')
  if (Math.abs(found.change) < STEADY) return t('retention.heatmap.steady', { later })
  return t(found.change > 0 ? 'retention.heatmap.more' : 'retention.heatmap.fewer', {
    date: dateLabel(found.since),
    share: format.percent(Math.abs(found.change)),
    later,
  })
})

const WIDTH = 280
const HEIGHT = 112
/** The chart's top: the highest of the averages and the drawn cohort, never below 1%. */
const chartMax = computed(() =>
  Math.max(
    0.01,
    ...props.result.averages.slice(1).map((average) => ratio(average) ?? 0),
    ...(active.value?.cells.map((cell) => cell.rate ?? 0) ?? [])
  )
)
const averageLine = computed(() =>
  pointList(
    curvePoints(
      props.result.averages.slice(1).map((average, index) => ({ offset: index + 1, rate: ratio(average) })),
      map.value.columns,
      chartMax.value,
      WIDTH,
      HEIGHT
    )
  )
)
const cohortCurve = computed(() => {
  const cells = active.value?.cells ?? []
  const at = (list: typeof cells) => curvePoints(list, map.value.columns, chartMax.value, WIDTH, HEIGHT)
  const complete = cells.filter((cell) => cell.complete)
  const lastComplete = complete.at(-1)
  const open = cells.filter((cell) => !cell.complete)
  return {
    points: at(cells),
    solid: pointList(at(complete)),
    dashed: open.length > 0 ? pointList(at(lastComplete ? [lastComplete, ...open] : open)) : '',
  }
})

const cohortName = (local: string) =>
  t(week.value ? 'retention.heatmap.cohortWeek' : 'retention.heatmap.cohortDay', { date: dateLabel(local) })

const sessionsLink = computed(() => {
  const cell = selectedCell.value
  if (!cell) return null
  const state: ExplorationState = {
    ...sessionListState(props.site.id, props.timezone),
    filter: props.state.filter,
    range: { kind: 'days', start: localDate(cell.start, props.timezone), end: localDate(cell.end - 1, props.timezone) },
  }
  return { name: 'sessions', params: { siteId: props.site.id }, query: { s: encodeState(state) } } as const
})
const intervalText = (start: number, end: number) =>
  new Intl.DateTimeFormat(locale.value, { month: 'short', day: 'numeric', timeZone: props.timezone }).formatRange(start, end - 1)

const onSelect = (position: GridPosition) => {
  const same = selected.value?.row === position.row && selected.value.column === position.column
  selected.value = same ? null : position
}
</script>

<template>
  <StageView :label="t('retention.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">
          {{ headline === null ? '–' : format.percent(headline) }}
        </p>
        <p class="mt-3 text-[0.9375rem] text-muted">
          {{ t(week ? 'retention.heatmap.headlineWeek' : 'retention.heatmap.headlineDay') }}
        </p>
        <p
          v-if="change"
          class="mt-1 text-sm font-medium tabular-nums"
          :class="Math.abs(change.change) < STEADY ? 'text-muted' : change.change > 0 ? 'text-success' : 'text-destructive'"
        >
          {{ t('retention.heatmap.change', { change: signedPercent(change.change) }) }}
        </p>

        <figure class="m-0 mt-6">
          <figcaption class="sr-only">{{ t('retention.heatmap.chart') }}</figcaption>
          <div class="flex gap-2">
            <div
              class="flex flex-col justify-between text-xs text-muted tabular-nums"
              aria-hidden="true"
            >
              <span>{{ format.percent(chartMax) }}</span>
              <span>{{ format.percent(0) }}</span>
            </div>
            <svg
              :viewBox="`-4 -4 ${WIDTH + 8} ${HEIGHT + 8}`"
              class="h-auto w-full overflow-visible"
              aria-hidden="true"
            >
              <line
                x1="0"
                :y1="HEIGHT"
                :x2="WIDTH"
                :y2="HEIGHT"
                class="stroke-hairline"
                stroke-width="1"
              />
              <polyline
                :points="averageLine"
                fill="none"
                stroke-width="2"
                stroke-linejoin="round"
                stroke-linecap="round"
                class="stroke-series-comparison"
              />
              <template v-if="active">
                <polyline
                  :points="cohortCurve.solid"
                  fill="none"
                  stroke-width="2.5"
                  stroke-linejoin="round"
                  stroke-linecap="round"
                  class="stroke-series-primary"
                />
                <polyline
                  v-if="cohortCurve.dashed"
                  :points="cohortCurve.dashed"
                  fill="none"
                  stroke-width="2"
                  stroke-dasharray="3 4"
                  stroke-linecap="round"
                  class="stroke-series-primary"
                />
                <circle
                  v-for="point in cohortCurve.points"
                  :key="point.x"
                  :cx="point.x"
                  :cy="point.y"
                  r="3"
                  class="fill-series-primary"
                />
              </template>
            </svg>
          </div>
          <p
            class="mt-2 flex justify-between gap-2 pl-8 text-sm text-muted tabular-nums"
            aria-hidden="true"
          >
            <span>{{ t(unit, { offset: 1 }) }}</span>
            <span v-if="map.columns > 1">{{ t(unit, { offset: map.columns }) }}</span>
          </p>
          <ul class="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-sm">
            <li class="flex items-center gap-2 text-muted">
              <span
                class="h-0.5 w-4 rounded-full bg-series-comparison"
                aria-hidden="true"
              />
              {{ t('retention.table.average') }}
            </li>
            <li
              v-if="active"
              class="flex items-center gap-2 font-medium text-primary"
            >
              <span
                class="h-0.5 w-4 rounded-full bg-series-primary"
                aria-hidden="true"
              />
              {{ cohortName(active.local) }}
            </li>
          </ul>
        </figure>
      </div>

      <div class="grid gap-4 pt-5">
        <p
          v-if="takeaway"
          class="text-[0.9375rem] text-primary"
        >
          {{ takeaway }}
        </p>
        <div
          v-if="selectedRow && selectedCell"
          class="grid gap-1 text-sm"
          aria-live="polite"
        >
          <p class="font-medium text-primary">
            {{ cohortName(selectedRow.local) }} · {{ t(unit, { offset: selectedCell.offset }) }}
          </p>
          <p class="text-muted">
            {{
              t('retention.heatmap.selected', {
                active: format.integer(selectedCell.active),
                size: format.integer(selectedRow.size),
                rate: selectedCell.rate === null ? '–' : format.percent(selectedCell.rate),
              })
            }}
          </p>
          <RouterLink
            v-if="sessionsLink"
            :to="sessionsLink"
            class="mt-1 font-semibold text-accent-text underline-offset-2 hover:underline focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            {{ t('retention.heatmap.sessions', { range: intervalText(selectedCell.start, selectedCell.end) }) }}
          </RouterLink>
          <p class="text-xs text-muted">{{ t('retention.heatmap.sessionsHint') }}</p>
        </div>
        <p
          v-else
          class="text-sm text-muted"
        >
          {{ t('retention.heatmap.hint') }}
        </p>
      </div>

      <p class="mt-auto pt-6 text-xs text-muted">{{ t('retention.heatmap.population') }}</p>
    </template>

    <CohortHeatmap
      :map="map"
      :granularity="state.granularity"
      :highlight="activeRow"
      :selected="selected"
      @point="(row) => (pointed = row)"
      @select="onSelect"
    >
      <template #toolbar>
        <slot name="toolbar" />
      </template>
    </CohortHeatmap>
  </StageView>
</template>
