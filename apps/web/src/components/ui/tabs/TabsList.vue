<script setup lang="ts">
import type { TabsListProps } from 'reka-ui'
import { TabsList } from 'reka-ui'
import type { HTMLAttributes } from 'vue'
import { computed } from 'vue'

import { segmentedTrack } from '~/components/ui/segmented'
import { cn } from '~/lib/utils'

const props = defineProps<TabsListProps & { class?: HTMLAttributes['class'] }>()

const delegatedProps = computed(() => {
  const { class: _, ...delegated } = props

  return delegated
})
</script>

<template>
  <TabsList
    v-bind="delegatedProps"
    :class="
      cn(
        segmentedTrack,
        'scroll-fade-x flex-nowrap items-center justify-start',
        props.class
      )
    "
  >
    <slot />
  </TabsList>
</template>
