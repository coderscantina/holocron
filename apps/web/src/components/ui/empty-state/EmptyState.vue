<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import Icon from '~/components/Icon.vue'
import { cn } from '~/lib/utils'

/**
 * The shape every "there is nothing here" moment takes: a mark, one line of
 * what is missing, one line of what to do, and the action itself.
 *
 * `tone` covers the three cases that look alike and mean different things:
 * nothing yet (`empty`), nothing matching (`empty` with a different message),
 * and something went wrong (`error`).
 */
const props = defineProps<{
  icon?: string
  title: string
  description?: string
  tone?: 'empty' | 'error'
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <div
    :class="
      cn(
        'flex flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-border px-6 py-12 max-sm:px-4 text-center',
        props.class
      )
    "
  >
    <Icon
      v-if="icon"
      :name="icon"
      size="28"
      :class="tone === 'error' ? 'text-destructive' : 'text-muted'"
      aria-hidden="true"
    />
    <div class="grid gap-1">
      <p class="font-medium text-primary">{{ title }}</p>
      <p
        v-if="description"
        class="max-w-prose text-sm text-balance text-muted"
      >
        {{ description }}
      </p>
    </div>
    <div
      v-if="$slots.default"
      class="mt-1 flex flex-wrap items-center justify-center gap-2"
    >
      <slot />
    </div>
  </div>
</template>
