<script setup lang="ts">
/**
 * A small bar after a value, so the reader sees its size before reading the number. `value` is
 * the share of the column's fixed scale, clamped to 0..1; a fixed scale keeps one length
 * meaning the same on every page and in every order.
 */
const props = defineProps<{
  value: number
}>()
</script>

<template>
  <span
    class="relative inline-block h-1.5 w-16 shrink-0 overflow-hidden rounded-[3px] bg-fill-strong"
    aria-hidden="true"
  >
    <span
      class="absolute inset-y-0 left-0 rounded-[3px] bg-series-primary"
      :style="{ width: `${Math.min(1, Math.max(0, props.value)) * 100}%` }"
    />
  </span>
</template>
