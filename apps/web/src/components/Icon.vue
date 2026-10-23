<script setup lang="ts">
import { Icon as IconifyIcon } from '@iconify/vue'
import type { HTMLAttributes } from 'vue'

/**
 * Renders an Iconify icon by name, e.g. `lucide:circle-user`. Names are
 * resolved at runtime, which is what lets an icon be stored as a string in
 * the database and picked from a list; a static component import cannot.
 * `@iconify-json/lucide` ships the set locally, so nothing is fetched.
 */
const props = defineProps<{
  name?: string | null
  size?: string | number
  class?: HTMLAttributes['class']
}>()

const iconSize = computed(() => {
  if (!props.size) return '1rem'
  return typeof props.size === 'number' ? `${props.size}px` : props.size
})
</script>

<template>
  <IconifyIcon
    v-if="name"
    :icon="name"
    :width="iconSize"
    :height="iconSize"
    :class="props.class"
  />
</template>
