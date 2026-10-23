<script setup lang="ts">
import type { TabsTriggerProps } from 'reka-ui'
import { TabsTrigger, useForwardProps } from 'reka-ui'
import type { HTMLAttributes } from 'vue'
import { computed } from 'vue'

import { segmentedItem } from '~/components/ui/segmented'
import { cn } from '~/lib/utils'

const props = defineProps<TabsTriggerProps & { class?: HTMLAttributes['class'] }>()

const delegatedProps = computed(() => {
  const { class: _, ...delegated } = props

  return delegated
})

const forwardedProps = useForwardProps(delegatedProps)
</script>

<template>
  <TabsTrigger
    v-bind="forwardedProps"
    :class="
      cn(
        segmentedItem({ size: 'md' }),
        props.class
      )
    "
  >
    <slot />
  </TabsTrigger>
</template>
