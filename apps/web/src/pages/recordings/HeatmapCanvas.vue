<script setup lang="ts">
import type { HeatmapCell } from '~/generated'

/**
 * Click density drawn on an outline of the page: one rectangle per cell, stronger where more
 * clicks landed. The page itself is not shown, so nothing captured is rendered here.
 */
const props = defineProps<{
  cells: HeatmapCell[]
  columns: number
  rowHeight: number
  /** Width the outline stands for, in page pixels. */
  width: number
  label: string
}>()

/** At least one screen tall, and tall enough for the deepest click. */
const height = computed(() => {
  const deepest = props.cells.reduce((max, cell) => Math.max(max, cell.row), 0)
  return Math.max(Math.round(props.width * 0.625), (deepest + 2) * props.rowHeight)
})
const max = computed(() => props.cells.reduce((top, cell) => Math.max(top, cell.count), 1))
const cellWidth = computed(() => props.width / props.columns)
</script>

<template>
  <svg
    :viewBox="`0 0 ${width} ${height}`"
    role="img"
    :aria-label="label"
    class="block h-auto w-full rounded-md border border-hairline bg-fill"
  >
    <rect
      v-for="cell in cells"
      :key="`${cell.column}-${cell.row}`"
      :x="cell.column * cellWidth"
      :y="cell.row * rowHeight"
      :width="cellWidth"
      :height="rowHeight"
      class="fill-series-primary"
      :fill-opacity="0.15 + 0.85 * (cell.count / max)"
    />
  </svg>
</template>
