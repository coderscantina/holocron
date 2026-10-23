<script setup lang="ts" generic="Row extends ReportBarRow">
import type { ReportBarRow } from './types'

/**
 * A report result as one semantic table whose first column draws each row's share as a bar.
 * The table is the chart's text form, so screen readers and the bars read the same values.
 * It scrolls inside its own labelled region on narrow screens. It has no frame of its own: put it
 * in a flush `Panel`.
 */
defineProps<{
  caption: string
  /** Header of the label column, then of each value column. */
  columns: string[]
  rows: Row[]
}>()

defineSlots<{ actions?: (props: { row: Row }) => unknown }>()

const regionId = useId()
</script>

<template>
  <div
    :id="regionId"
    role="region"
    tabindex="0"
    :aria-label="caption"
    class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
  >
    <table class="w-full text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
      <caption class="sr-only">{{ caption }}</caption>
      <thead>
        <tr class="h-table-head bg-fill text-[0.875rem] font-medium text-muted">
          <th
            v-for="(column, index) in columns"
            :key="index"
            scope="col"
            class="px-4 font-medium whitespace-nowrap"
            :class="index === 0 ? 'text-left' : 'text-right'"
          >
            {{ column }}
          </th>
          <th
            v-if="$slots.actions"
            scope="col"
            class="w-10 px-2"
          >
            <span class="sr-only">{{ columns[0] }}</span>
          </th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="row in rows"
          :key="row.key"
          class="h-table-row border-t border-divider"
        >
          <th
            scope="row"
            class="min-w-48 px-4 py-2 text-left font-normal"
          >
            <span
              class="block max-w-[32rem] truncate"
              :class="row.muted ? 'text-muted italic' : 'text-primary'"
              :title="row.label"
            >
              {{ row.label }}
            </span>
            <span
              class="mt-1 block h-1.5 rounded-full bg-fill-strong"
              aria-hidden="true"
            >
              <span
                class="block h-full rounded-full bg-accent"
                :style="{ width: `${Math.max(0, Math.min(1, row.share)) * 100}%` }"
              />
            </span>
          </th>
          <td
            v-for="(value, index) in row.values"
            :key="index"
            class="px-4 py-2 text-right whitespace-nowrap tabular-nums"
            :class="index === 0 ? 'font-semibold text-primary' : 'text-foreground'"
          >
            {{ value }}
          </td>
          <td
            v-if="$slots.actions"
            class="py-2 pr-3 pl-2 text-right"
          >
            <slot
              name="actions"
              :row="row"
            />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
