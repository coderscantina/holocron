<script lang="ts" setup>
import { reactiveOmit } from '@vueuse/core'
import type { DialogContentEmits, DialogContentProps } from 'reka-ui'
import {
  DialogClose,
  DialogContent,
  DialogOverlay,
  DialogPortal,
  useForwardPropsEmits,
} from 'reka-ui'
import type { HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import { overlayClass } from '~/components/ui/overlay'
import { cn } from '~/lib/utils'

import type { SheetVariants } from './variants'
import { sheetVariants } from './variants'

interface SheetContentProps extends DialogContentProps {
  class?: HTMLAttributes['class']
  side?: SheetVariants['side']
}

defineOptions({
  inheritAttrs: false,
})

const props = defineProps<SheetContentProps>()

const emits = defineEmits<DialogContentEmits>()

const delegatedProps = reactiveOmit(props, 'class', 'side')

const forwarded = useForwardPropsEmits(delegatedProps, emits)
</script>

<template>
  <DialogPortal>
    <DialogOverlay :class="cn(overlayClass, 'z-40')" />
    <DialogContent
      :class="cn(sheetVariants({ side }), props.class)"
      v-bind="{ ...forwarded, ...$attrs }"
    >
      <slot />

      <DialogClose
        class="absolute right-4 top-4 rounded-sm opacity-70 ring-offset-surface transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none data-[state=open]:bg-secondary"
      >
        <Icon name="lucide:x" />
      </DialogClose>
    </DialogContent>
  </DialogPortal>
</template>
