<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import Tooltip from './Tooltip.vue'
import TooltipContent from './TooltipContent.vue'
import TooltipProvider from './TooltipProvider.vue'
import TooltipTrigger from './TooltipTrigger.vue'

// Attributes go to the trigger, not to the provider. The provider renders a
// fragment, so anything left to fall through to it is dropped — including the
// props a menu trigger hands down when this tooltip is its `as-child` target.
defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    tooltip: string
    delayDuration?: number
    side?: 'top' | 'right' | 'bottom' | 'left'
    disabled?: boolean
    /**
     * Render the slot as the trigger instead of wrapping it in a button. For
     * anything that is already interactive — a link, a button that another
     * component drives — where a second button would be invalid markup and a
     * second tab stop.
     */
    asChild?: boolean
    class?: HTMLAttributes['class']
  }>(),
  {
    delayDuration: 300,
    side: 'top',
    disabled: false,
    asChild: false,
    class: '',
  }
)
</script>

<template>
  <TooltipProvider :delay-duration="delayDuration">
    <Tooltip>
      <TooltipTrigger
        v-bind="$attrs"
        :as-child="asChild"
        :type="asChild ? undefined : 'button'"
        :class="props.class"
      >
        <slot />
      </TooltipTrigger>
      <TooltipContent
        v-if="!disabled"
        :side="side"
      >
        <p>{{ tooltip }}</p>
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>
