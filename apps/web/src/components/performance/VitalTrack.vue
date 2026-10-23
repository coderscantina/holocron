<script setup lang="ts">
import type { Metric } from '~/generated'
import { track, type Rating, type Vital } from '~/lib/vitals'

/**
 * One vital on its rating scale: the good, needs-improvement and poor bands, p50 and p95 as thin
 * ticks and p75, the value the rating uses, as a marker in its rating's colour.
 * Decorative: the card states the same values and rating in words.
 */
const props = defineProps<{
  vital: Vital
  /** p50, p75 and p95. */
  values: readonly (Metric | undefined)[]
  rating: Rating | null
}>()

const scale = computed(() => track(props.vital, props.values))
const ticks = computed(() => [scale.value.points[0], scale.value.points[2]].filter((point) => point != null))
const marker = computed(() => scale.value.points[1] ?? null)
const tone: Record<Rating, string> = { good: 'bg-success', improve: 'bg-warning', poor: 'bg-destructive' }
</script>

<template>
  <div
    class="relative h-3.5"
    aria-hidden="true"
  >
    <div class="absolute inset-x-0 top-1/2 flex h-1.5 -translate-y-1/2 gap-0.5">
      <span
        class="shrink rounded-l-full bg-success/35"
        :style="{ width: `${scale.good}%` }"
      />
      <span
        class="shrink bg-warning/35"
        :style="{ width: `${scale.improve - scale.good}%` }"
      />
      <span class="flex-1 rounded-r-full bg-destructive/35" />
    </div>
    <span
      v-for="(tick, index) in ticks"
      :key="index"
      class="absolute top-1/2 h-2.5 w-0.5 -translate-1/2 rounded-full bg-foreground/55"
      :style="{ left: `${tick}%` }"
    />
    <span
      v-if="marker !== null"
      class="absolute top-0 h-3.5 w-1 -translate-x-1/2 rounded-full ring-2 ring-card"
      :class="rating ? tone[rating] : 'bg-primary'"
      :style="{ left: `${marker}%` }"
    />
  </div>
</template>
