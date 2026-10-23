<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * The column row. No fill: the stronger rule under it is what separates the labels from the
 * first record, so the header does not compete with the data. The row takes no hover, because
 * it is not a record.
 */
const props = defineProps<{
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <thead
    role="rowgroup"
    :class="cn('[&_tr]:border-b! [&_tr]:border-b-divider [&_tr]:bg-fill [&_tr:hover]:bg-fill', props.class)"
  >
    <slot />
  </thead>
</template>
