<script setup lang="ts">
import { useTopBarTarget } from '~/components/app/AppTopBarSlot'

/**
 * Mounts its content in the top bar's scope outlet while the page is mounted. `summary` is what
 * the bar shows in place of the chips when they do not fit, for example "Today · vs typical · 2
 * filters"; the chips then render in that chip's popover.
 */
const props = defineProps<{ summary: string }>()

const target = useTopBarTarget(() => props.summary)
</script>

<template>
  <Teleport
    v-if="target"
    :to="target"
  >
    <slot />
  </Teleport>
</template>
