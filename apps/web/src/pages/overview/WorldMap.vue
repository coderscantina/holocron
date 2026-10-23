<script setup lang="ts">
import CountryFlag from '~/components/CountryFlag.vue'
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { Cut, ExplorationState } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { include } from '~/services/exploration'

import { WORLD_HEIGHT, WORLD_SHAPES, WORLD_WIDTH } from './world-shapes'

/**
 * Visitor-days by country as a shaded map. Five steps on a square-root scale of the largest
 * country, so a dominant home market leaves the rest visible. Hover names a country; a click
 * keeps only it. The countries panel beside it holds the same figures for keyboard and
 * screen reader use, so the map itself is one labelled image.
 */
const props = defineProps<{
  state: ExplorationState
  cut: Cut | null
}>()

const emit = defineEmits<{ commit: [state: ExplorationState] }>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()

const STEPS = 5

const countries = computed(() => {
  const values = new Map<string, number>()
  for (const row of props.cut?.rows ?? []) {
    const cell = row.key[0]
    const metric = row.values[0]
    if (cell?.t === 'string' && metric?.t === 'count') values.set(cell.v, metric.v)
  }
  return values
})
const total = computed(() => {
  const metric = props.cut?.totals[0]
  return metric?.t === 'count' ? metric.v : 0
})
const max = computed(() => Math.max(1, ...countries.value.values()))

const step = (code: string): number => {
  const value = countries.value.get(code)
  if (!value) return 0
  return Math.max(1, Math.ceil(Math.sqrt(value / max.value) * STEPS))
}

/** Step 0 is the land without visits; the others mix the series colour into it. */
const fill = (level: number) =>
  level === 0
    ? 'var(--secondary)'
    : `color-mix(in oklch, var(--series-primary) ${20 + (level - 1) * 20}%, var(--secondary))`

const shapes = computed(() =>
  Object.entries(WORLD_SHAPES).map(([code, d]) => ({ code, d, fill: fill(step(code)) }))
)

const hovered = ref<{ code: string; x: number; y: number } | null>(null)
const hoveredText = computed(() => {
  const current = hovered.value
  if (!current) return null
  const value = countries.value.get(current.code) ?? 0
  return t('overview.panels.countries.tooltip', {
    country: format.cellLabel('country', { t: 'string', v: current.code }),
    value: format.metric({ t: 'count', v: value }),
    share: numbers.percent(total.value > 0 ? value / total.value : 0),
  })
})

const codeAt = (event: Event) => (event.target instanceof SVGPathElement ? event.target.dataset.code : undefined)

const onMove = (event: PointerEvent) => {
  const code = codeAt(event)
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  hovered.value = code ? { code, x: event.clientX - box.left, y: event.clientY - box.top } : null
}

const onClick = (event: MouseEvent) => {
  const code = codeAt(event)
  if (code && countries.value.has(code)) emit('commit', include(props.state, 'country', { t: 'string', v: code }))
}
</script>

<template>
  <section class="relative flex min-w-0 flex-col justify-center bg-card px-shell-gutter-x py-5">
    <div
      v-if="cut"
      class="relative"
      @pointermove="onMove"
      @pointerleave="hovered = null"
      @click="onClick"
    >
      <svg
        :viewBox="`0 0 ${WORLD_WIDTH} ${WORLD_HEIGHT}`"
        class="h-auto w-full"
        role="img"
        :aria-label="t('overview.panels.countries.map')"
      >
        <path
          v-for="shape in shapes"
          :key="shape.code"
          :d="shape.d"
          :data-code="shape.code"
          :style="{ fill: shape.fill }"
          class="stroke-card stroke-[0.6] transition-[fill] duration-300"
          :class="[
            countries.has(shape.code) && 'cursor-pointer',
            hovered?.code === shape.code && 'stroke-primary stroke-1',
          ]"
        />
      </svg>
      <div
        v-if="hovered && hoveredText"
        class="pointer-events-none absolute z-10 flex -translate-x-1/2 -translate-y-[calc(100%+10px)] items-center gap-2 rounded-md border border-popover-border bg-popover px-2 py-1 text-sm whitespace-nowrap text-popover-foreground shadow-soft-lg"
        :style="{ left: `${hovered.x}px`, top: `${hovered.y}px` }"
      >
        <CountryFlag :code="hovered.code" />
        {{ hoveredText }}
      </div>
    </div>
    <Skeleton
      v-else
      class="aspect-[1000/440] w-full"
    />

    <div
      class="mt-2 flex items-center gap-2 text-sm text-muted"
      aria-hidden="true"
    >
      <span>{{ t('overview.panels.countries.legend') }}</span>
      <span class="flex gap-0.5">
        <span
          v-for="level in STEPS"
          :key="level"
          class="h-2 w-5 rounded-xs"
          :style="{ background: fill(level) }"
        />
      </span>
      <span>{{ t('overview.panels.countries.legendMore') }}</span>
    </div>
  </section>
</template>
