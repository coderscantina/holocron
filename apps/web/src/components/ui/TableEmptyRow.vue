<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ui/TableEmptyRow.vue. The label is
// required, since Holocron has no generic "no results" copy.
import { computed, type Component } from 'vue'

import { TableCell, TableRow } from '~/components/ui/table'

/**
 * The "nothing here" row. It keeps the table's own edges rather than painting
 * a panel inside it, so an empty list still reads as the same list.
 */
const props = withDefaults(
  defineProps<{
    colspan?: number
    // A component, never an icon name: `<Component :is>` would resolve a string
    // as a tag name and render an unknown element.
    icon?: Component
    label: string
    description?: string
  }>(),
  {
    colspan: 3,
    icon: undefined,
    description: undefined,
  }
)

const iconComponent = computed(() => (typeof props.icon === 'string' ? undefined : props.icon))
</script>

<template>
  <TableRow class="hover:bg-transparent">
    <TableCell
      :colspan="colspan"
      class="py-14 text-center select-none"
    >
      <div class="flex flex-col items-center justify-center gap-3">
        <Component
          :is="iconComponent"
          v-if="iconComponent"
          class="w-20 text-muted opacity-60"
        />
        <p class="font-medium text-primary">{{ label }}</p>
        <p
          v-if="description"
          class="max-w-sm text-sm text-balance text-muted"
        >
          {{ description }}
        </p>
        <slot name="actions" />
      </div>
    </TableCell>
  </TableRow>
</template>
