<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <tr
    role="row"
    :class="
      cn(
        'border-b border-b-divider transition-colors duration-150 ease-butter last:border-b-0 hover:bg-fill/50 data-[state=selected]:bg-fill',
        props.class
      )
    "
  >
    <slot />
  </tr>
</template>
