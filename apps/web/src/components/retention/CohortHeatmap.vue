<script setup lang="ts">
import { useFormat } from '~/composables/useFormat'
import type { Granularity } from '~/generated'
import { heatFill, moveInGrid, type GridPosition, type HeatCell, type HeatRow, type Heatmap } from '~/lib/retention-heatmap'
import { useI18n } from '~/plugins/i18n'

/**
 * Cohorts as rows and the periods after their first visit as columns, each cell shaded by its
 * share of the cohort that came back. One tab stop: arrow keys, Home and End move between
 * cells, Enter or a click selects one. Pointing at or focusing a cell reports its row, so the
 * page can draw that cohort; the other rows dim.
 */
const props = defineProps<{
  map: Heatmap
  granularity: Granularity
  /** Row drawn in full while others dim. */
  highlight: number | null
  selected: GridPosition | null
}>()

const emit = defineEmits<{
  point: [row: number | null]
  select: [position: GridPosition]
}>()

const { t, locale } = useI18n()
const format = useFormat()
const root = useTemplateRef('root')

const week = computed(() => props.granularity === 'week')
/** Beyond this many columns the cells are too narrow for their percentage. */
const showValues = computed(() => props.map.columns <= 16)

const firstCell = computed<GridPosition | null>(() => {
  const row = props.map.rows.findIndex((entry) => entry.cells.length > 0)
  return row < 0 ? null : { row, column: 0 }
})
const cursor = ref<GridPosition | null>(null)
const tabStop = computed(() => cursor.value ?? props.selected ?? firstCell.value)
watch(
  () => props.map,
  () => {
    cursor.value = null
  }
)

const isAt = (position: GridPosition | null, row: number, column: number) =>
  position !== null && position.row === row && position.column === column

const cohortLabel = (row: HeatRow) =>
  new Intl.DateTimeFormat(locale.value, { month: 'short', day: 'numeric', timeZone: 'UTC' }).format(Date.parse(`${row.local}T00:00:00Z`))

const cellLabel = (row: HeatRow, cell: HeatCell) =>
  [
    t(week.value ? 'retention.heatmap.cohortWeek' : 'retention.heatmap.cohortDay', { date: cohortLabel(row) }),
    t(week.value ? 'retention.table.week' : 'retention.table.day', { offset: cell.offset }),
    cell.rate === null
      ? '–'
      : t('retention.heatmap.cellValue', { rate: format.percent(cell.rate), active: format.integer(cell.active), size: format.integer(row.size) }),
    cell.complete ? '' : t('retention.table.incomplete'),
  ]
    .filter(Boolean)
    .join(', ')

const focusCell = (position: GridPosition) => {
  cursor.value = position
  void nextTick(() =>
    root.value?.querySelector<HTMLElement>(`[data-cell="${position.row}-${position.column}"]`)?.focus()
  )
}

const onKey = (event: KeyboardEvent, position: GridPosition) => {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault()
    emit('select', position)
    return
  }
  const next = moveInGrid(
    props.map.rows.map((row) => row.cells.length),
    position,
    event.key
  )
  if (!next) return
  event.preventDefault()
  focusCell(next)
}

const onFocus = (position: GridPosition) => {
  cursor.value = position
  emit('point', position.row)
}
</script>

<template>
  <div
    ref="root"
    class="flex h-full min-h-0 flex-col"
  >
    <div class="mb-3 flex flex-wrap-reverse items-end justify-between gap-x-4 gap-y-3">
      <p
        class="text-sm text-muted"
        aria-hidden="true"
      >
        {{ t(week ? 'retention.heatmap.weeksAfter' : 'retention.heatmap.daysAfter') }}
      </p>
      <slot name="toolbar" />
    </div>
    <div
      role="grid"
      :aria-label="t('retention.heatmap.label')"
      :aria-rowcount="map.rows.length + 1"
      class="grid min-h-0 flex-1 [--heat-max:80%] dark:[--heat-max:55%] auto-rows-[minmax(2.25rem,3.75rem)] content-start gap-0.5 overflow-auto"
      :style="{
        gridTemplateColumns: `auto auto repeat(${map.columns}, minmax(${showValues ? '2.75rem' : '1.25rem'}, 1fr))`,
        gridTemplateRows: 'auto',
      }"
      @pointerleave="emit('point', null)"
    >
      <div
        role="row"
        class="contents"
      >
        <div
          role="columnheader"
          :style="{ gridRow: 1 }"
          class="sticky top-0 z-10 bg-surface pr-3 pb-1 text-left text-xs font-medium text-muted"
        >
          {{ t('retention.table.cohort') }}
        </div>
        <div
          role="columnheader"
          :style="{ gridRow: 1 }"
          class="sticky top-0 z-10 bg-surface pr-3 pb-1 text-right text-xs font-medium text-muted"
        >
          {{ t('retention.heatmap.people') }}
        </div>
        <div
          v-for="column in map.columns"
          :key="column"
          role="columnheader"
          :aria-label="t(week ? 'retention.table.week' : 'retention.table.day', { offset: column })"
          :style="{ gridRow: 1 }"
          class="sticky top-0 z-10 bg-surface pb-1 text-center text-xs font-medium text-muted tabular-nums"
        >
          {{ column }}
        </div>
      </div>

      <div
        v-for="(row, rowIndex) in map.rows"
        :key="row.start"
        role="row"
        class="contents"
      >
        <div
          role="rowheader"
          :style="{ gridRow: rowIndex + 2, gridColumn: 1 }"
          class="flex items-center pr-3 text-sm whitespace-nowrap transition-colors"
          :class="highlight === rowIndex ? 'font-semibold text-primary' : 'text-card-foreground'"
          @pointerenter="emit('point', rowIndex)"
        >
          {{ cohortLabel(row) }}
        </div>
        <div
          role="gridcell"
          :style="{ gridRow: rowIndex + 2, gridColumn: 2 }"
          class="flex items-center justify-end pr-3 text-sm text-muted tabular-nums"
          @pointerenter="emit('point', rowIndex)"
        >
          {{ format.integer(row.size) }}
        </div>
        <div
          v-for="(cell, column) in row.cells"
          :key="cell.offset"
          role="gridcell"
          :data-cell="`${rowIndex}-${column}`"
          :tabindex="isAt(tabStop, rowIndex, column) ? 0 : -1"
          :aria-label="cellLabel(row, cell)"
          :aria-selected="isAt(selected, rowIndex, column)"
          class="flex cursor-pointer items-center justify-center rounded-[3px] text-xs tabular-nums outline-offset-1 focus-visible:outline-2 focus-visible:outline-ring motion-safe:transition-opacity"
          :class="[
            highlight !== null && highlight !== rowIndex ? 'opacity-55' : '',
            cell.complete ? 'text-primary' : 'text-muted outline-1 -outline-offset-1 outline-border-strong outline-dashed',
            isAt(selected, rowIndex, column) ? 'shadow-[inset_0_0_0_2px_var(--primary)]' : '',
          ]"
          :style="{ gridRow: rowIndex + 2, gridColumn: cell.offset + 2, backgroundColor: heatFill(cell.step) }"
          @pointerenter="emit('point', rowIndex)"
          @focus="onFocus({ row: rowIndex, column })"
          @blur="emit('point', null)"
          @keydown="onKey($event, { row: rowIndex, column })"
          @click="emit('select', { row: rowIndex, column })"
        >
          <span
            v-if="showValues"
            aria-hidden="true"
          >
            {{ cell.rate === null ? '–' : format.percent(cell.rate) }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
