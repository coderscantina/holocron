<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <tfoot
    role="rowgroup"
    :class="
      cn(
        'border-t border-t-divider font-medium text-primary [&>tr]:last:border-b-0 [&>tr:hover]:bg-transparent',
        props.class
      )
    "
  >
    <slot />
  </tfoot>
</template>
