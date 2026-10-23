<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn, initials } from '~/lib/utils'

import type { AvatarVariants } from './variants'
import { avatarVariants } from './variants'

const props = defineProps<{
  name: string
  avatar?: string | null
  borderColor?: string | null
  size?: AvatarVariants['size']
  class?: HTMLAttributes['class']
}>()

const letters = computed(() => initials(props.name))

const width = computed(() => {
  switch (props.size) {
    case 'sm':
      return 32
    case 'lg':
      return 96
    default:
      return 64
  }
})

const fontSize = computed(() => {
  switch (props.size) {
    case 'sm':
      return 'text-[8px] font-bold'
    default:
      return 'text-xs font-bold'
  }
})
</script>

<template>
  <div
    :class="
      cn(avatarVariants({ size }), props.borderColor ? 'border-2' : 'border-none', props.class)
    "
    :style="{ borderColor: props.borderColor || 'transparent' }"
  >
    <img
      v-if="props.avatar"
      :src="props.avatar"
      :alt="letters"
      :width="width"
      :height="width"
      loading="lazy"
      class="h-full w-full object-cover"
    />
    <span
      v-else
      :class="fontSize"
      >{{ letters }}</span
    >
  </div>
</template>
