<script setup lang="ts">
/**
 * The page title row at the top of the canvas: name, bullet, muted context, then the actions and
 * the freshness note at the right, with a hairline below. It spans the canvas edge to edge, so it
 * must be the first thing in the page. `eyebrow` adds small context after the description.
 */
defineProps<{
  title: string
  description?: string
}>()
</script>

<template>
  <div
    class="-mx-shell-page-x -mt-shell-gutter mb-6 flex min-h-16 flex-wrap items-center gap-x-3 gap-y-1 border-b border-hairline px-shell-page-x py-2"
  >
    <h1 class="min-w-0 truncate text-xl font-semibold tracking-[-0.01em] text-primary">{{ title }}</h1>
    <template v-if="description">
      <span
        class="text-muted/60 max-sm:hidden"
        aria-hidden="true"
      >•</span>
      <p class="min-w-0 truncate text-lg text-muted">{{ description }}</p>
    </template>
    <div
      v-if="$slots.eyebrow"
      class="flex flex-wrap items-center gap-2 text-xs text-muted"
    >
      <slot name="eyebrow" />
    </div>
    <div
      v-if="$slots.actions || $slots.freshness"
      class="ml-auto flex flex-wrap items-center justify-end gap-x-3 gap-y-2"
    >
      <slot name="actions" />
      <div
        v-if="$slots.freshness"
        class="flex items-center gap-2 text-sm text-muted"
      >
        <slot name="freshness" />
      </div>
    </div>
  </div>
</template>
