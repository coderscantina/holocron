<script setup lang="ts">
import type { HTMLAttributes } from 'vue'

import { cn } from '~/lib/utils'

/**
 * A block on the canvas: hairline edge, card tone, and an optional 56px header with the title, a
 * muted count or context after a bullet, and controls at the right end. The body is padded unless
 * `flush`, which suits a table or a chart that runs to the edges. The `#title` slot replaces the
 * heading, for a panel whose title is a switch; it brings its own heading then.
 */
const props = defineProps<{
  title?: string
  /** Short muted context after the title, for example "214 pages". Hidden in narrow panels. */
  meta?: string
  /** Heading id, so the body can point `aria-labelledby` at it. */
  id?: string
  flush?: boolean
  class?: HTMLAttributes['class']
}>()
</script>

<template>
  <section
    :aria-labelledby="title && id ? id : undefined"
    :class="cn('@container flex min-w-0 flex-col overflow-hidden rounded-panel border border-hairline bg-card text-card-foreground', props.class)"
  >
    <header
      v-if="title || $slots.actions"
      class="flex h-14 shrink-0 items-center gap-2.5 pr-3 pl-4.5"
    >
      <slot name="title">
        <h2
          v-if="title"
          :id="id"
          class="min-w-0 truncate text-[1.0625rem] font-semibold text-primary"
        >
          {{ title }}
        </h2>
      </slot>
      <span
        v-if="meta"
        class="hidden min-w-0 shrink-[100] truncate text-sm text-muted tabular-nums @md:inline"
      >
        · {{ meta }}
      </span>
      <div
        v-if="$slots.actions"
        class="ml-auto flex shrink-0 items-center gap-2"
      >
        <slot name="actions" />
      </div>
    </header>
    <div :class="flush ? 'min-w-0' : cn('min-w-0 px-4.5 pb-4.5', !(title || $slots.actions) && 'pt-4.5')">
      <slot />
    </div>
  </section>
</template>
