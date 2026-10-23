<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ui/table/Table.vue. Holocron's
// stylesheet has no stacked-table layout, so a wide table scrolls inside its box on a phone.
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A list in its own box: one border and one radius around itself, never inside a `Card`.
 * `bare` drops the box and the scroller, for a table inside a `Panel` that brings its own.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
  /** The accessible name. A table without one is "table" to a screen reader. */
  label?: string
  bare?: boolean
}>()
</script>

<template>
  <table
    v-if="bare"
    :aria-label="label"
    :class="cn('w-full caption-bottom text-sm', props.class)"
  >
    <slot />
  </table>
  <div
    v-else
    class="overflow-hidden rounded-panel border border-hairline bg-card"
  >
    <div class="scroll-fade-x relative w-full overflow-x-auto">
      <table
        :aria-label="label"
        :class="cn('w-full caption-bottom text-sm', props.class)"
      >
        <slot />
      </table>
    </div>
  </div>
</template>
