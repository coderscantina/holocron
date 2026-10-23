<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * Where a cell sits in the row's reading order. Every table has one `subject` column (what
 * the row is) and at most one `key` number (the measure it is read for); the rest is body
 * text, `secondary` context or plain `numeric` values.
 */
export type TableCellKind = 'body' | 'subject' | 'secondary' | 'numeric' | 'key'

const kinds: Record<TableCellKind, string> = {
  body: 'text-foreground',
  subject: 'font-medium text-primary',
  secondary: 'text-muted',
  numeric: 'text-right text-foreground tabular-nums',
  key: 'text-right text-primary tabular-nums',
}

/**
 * One cell. `label` is the column's name, repeated for the stacked layout a
 * phone gets: there the header row is off screen, so the cell has to say what
 * it is. It is real text rather than generated content, and it is hidden the
 * screen-reader way rather than removed, so a phone still announces it while
 * the record stays one line of values wide.
 *
 * The name is also written to `data-label`, which is what the stacked layout
 * tells a record's secondary fields from its subject and its actions by.
 *
 * A `#detail` slot adds a second, muted line under the value and makes the row a stacked
 * record: the source under an entry page, the OS under a browser.
 */
const props = withDefaults(
  defineProps<{
    class?: HTMLAttributes['class']
    label?: string
    kind?: TableCellKind
  }>(),
  { label: undefined, kind: 'body', class: undefined }
)
</script>

<template>
  <td
    role="cell"
    :data-label="label"
    :class="
      cn(
        'h-table-row border-divider px-4 py-2 align-middle not-first:border-l [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-0.5',
        kinds[kind],
        $slots.detail && 'h-table-row-stacked',
        props.class
      )
    "
  >
    <span
      v-if="label"
      class="sr-only sm:hidden"
      >{{ label }}</span
    >
    <template v-if="$slots.detail">
      <span class="block truncate">
        <slot />
      </span>
      <span class="block truncate text-xs font-normal text-muted">
        <slot name="detail" />
      </span>
    </template>
    <slot v-else />
  </td>
</template>
