<script setup lang="ts">
import {
  DialogClose,
  DialogContent,
  type DialogContentEmits,
  type DialogContentProps,
  DialogOverlay,
  DialogPortal,
  useForwardPropsEmits,
} from 'reka-ui'
import { computed, type HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import { overlayClass } from '~/components/ui/overlay'
import { cn } from '~/lib/utils'

const props = defineProps<DialogContentProps & { class?: HTMLAttributes['class'] }>()
const emits = defineEmits<DialogContentEmits>()

const delegatedProps = computed(() => {
  const { class: _, ...delegated } = props

  return delegated
})

const forwarded = useForwardPropsEmits(delegatedProps, emits)
</script>

<template>
  <DialogPortal>
    <DialogOverlay :class="cn(overlayClass, 'grid place-items-center overflow-y-auto')">
      <DialogContent
        :class="
          cn(
            'relative z-50 my-8 grid w-full max-w-lg gap-4 border border-border bg-card p-6 shadow-soft-lg max-sm:px-4 duration-200 sm:rounded-lg md:w-full',
            props.class
          )
        "
        v-bind="forwarded"
        @pointer-down-outside="
          (event) => {
            const originalEvent = event.detail.originalEvent
            const target = originalEvent.target as HTMLElement
            if (
              originalEvent.offsetX > target.clientWidth ||
              originalEvent.offsetY > target.clientHeight
            ) {
              event.preventDefault()
            }
          }
        "
      >
        <slot />

        <DialogClose
          class="absolute top-4 right-4 z-20 rounded-md p-0.5 transition-colors hover:bg-secondary"
        >
          <Icon name="lucide:x" />
          <span class="sr-only">Close</span>
        </DialogClose>
      </DialogContent>
    </DialogOverlay>
  </DialogPortal>
</template>
