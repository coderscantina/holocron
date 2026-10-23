<script setup lang="ts">
import { SelectIcon, SelectTrigger, type SelectTriggerProps, useForwardProps } from 'reka-ui'
import { computed, type HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import { cn } from '~/lib/utils'

const props = defineProps<SelectTriggerProps & { class?: HTMLAttributes['class'] }>()

const delegatedProps = computed(() => {
  const { class: _, ...delegated } = props

  return delegated
})

const forwardedProps = useForwardProps(delegatedProps)
</script>

<template>
  <SelectTrigger
    v-bind="forwardedProps"
    :class="
      cn(
        'flex h-9 w-full cursor-pointer items-center justify-between rounded-md border border-input-border bg-input px-3 py-2 text-start text-sm font-semibold whitespace-nowrap shadow-sm focus-visible:border-ring focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 data-[placeholder]:text-muted [&>span]:truncate',
        props.class
      )
    "
  >
    <slot />
    <SelectIcon as-child>
      <Icon
        name="lucide:chevron-down"
        class="shrink-0"
      />
    </SelectIcon>
  </SelectTrigger>
</template>
