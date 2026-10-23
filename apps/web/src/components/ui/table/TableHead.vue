<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A column label. Quieter than the data under it, but not faint: the weight and the rule under
 * the row are what separate the header from the first record. Columns have no dividers;
 * alignment and spacing keep them apart. `numeric` right-aligns the label over a number column.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
  numeric?: boolean
}>()
</script>

<template>
  <th
    scope="col"
    role="columnheader"
    :class="
      cn(
        'h-table-head px-4 text-left align-middle text-[0.875rem] font-medium whitespace-nowrap text-muted [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-0.5',
        numeric && 'text-right',
        props.class
      )
    "
  >
    <slot />
  </th>
</template>
