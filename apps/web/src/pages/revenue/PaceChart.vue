<script setup lang="ts">
import { useElementSize } from '@vueuse/core'

import { bucketOf } from '~/components/exploration/trend'
import type { Annotation, Granularity } from '~/generated'
import type { Pace } from '~/lib/revenue-summary'
import { useI18n } from '~/plugins/i18n'

/**
 * The running total of net revenue as a solid line, the comparison's running total as a gray
 * line, and after now the estimate as a dashed line with its band, ending at the server's
 * end-of-range estimate. Annotations mark their bucket. Hovering a bucket shows its values;
 * a table carries the same for assistive technology.
 */
const props = defineProps<{
  pace: Pace
  granularity: Granularity
  /** End of the range, Unix milliseconds, exclusive. */
  end: number
  annotations: Annotation[]
  /** Formats minor units of the chosen currency, compact for axis and end labels. */
  amount: (units: number, compact?: boolean) => string
  label: string
  /** What the gray line is, e.g. "Previous period"; `null` without a comparison. */
  previousLabel: string | null
}>()

const { t, locale } = useI18n()

const points = computed(() => props.pace.points)
const count = computed(() => Math.max(1, points.value.length))

const roundUp = (value: number) => {
  const magnitude = 10 ** Math.floor(Math.log10(value))
  return Math.ceil(value / magnitude) * magnitude
}
/** Zero to a round number above every line and band; refunds can take a running total below zero. */
const scale = computed(() => {
  const values = points.value.flatMap((point) => [point.actual ?? 0, point.previous ?? 0, point.estimate?.high ?? 0, point.estimate?.low ?? 0])
  const highest = Math.max(0, ...values)
  const lowest = Math.min(0, ...values)
  const top = highest > 0 ? roundUp(highest) : 1
  const bottom = lowest < 0 ? -roundUp(-lowest) : 0
  return { top, bottom, span: top - bottom }
})
const level = (value: number) => ((value - scale.value.bottom) / scale.value.span) * 100
const ticks = computed(() => [scale.value.top, scale.value.top / 2, 0].map((value) => ({ value, text: props.amount(value, true) })))

/** Running totals are read at the end of their bucket; the line starts at zero before the first. */
const x = (index: number) => ((index + 1) / count.value) * 100
const y = (value: number) => 100 - level(value)
const line = (values: (number | null)[]) =>
  values.reduce((path, value, index) => (value === null ? path : `${path} L${x(index)},${y(value)}`), `M0,${y(0)}`)

const actualPath = computed(() => line(points.value.map((point) => point.actual)))
const previousPath = computed(() => (props.previousLabel ? line(points.value.map((point) => point.previous)) : null))
const current = computed(() => points.value.findLastIndex((point) => point.actual !== null))
const estimated = computed(() => points.value.flatMap((point, index) => (point.estimate ? [{ index, ...point.estimate }] : [])))
const estimatePath = computed(() => {
  if (estimated.value.length === 0 || current.value < 0) return null
  const from = `M${x(current.value)},${y(props.pace.actual)}`
  return estimated.value.reduce((path, point) => `${path} L${x(point.index)},${y(point.value)}`, from)
})
const bandPath = computed(() => {
  if (estimated.value.length === 0 || current.value < 0) return null
  const from = `M${x(current.value)},${y(props.pace.actual)}`
  const high = estimated.value.map((point) => `L${x(point.index)},${y(point.high)}`).join(' ')
  const low = [...estimated.value].reverse().map((point) => `L${x(point.index)},${y(point.low)}`).join(' ')
  return `${from} ${high} ${low} Z`
})

/** End labels at the right edge: the estimate (or the last running total) and the comparison's total. */
const ends = computed(() => {
  const last = points.value.at(-1)
  const out: { key: string; value: number; text: string; tone: string }[] = []
  if (last?.estimate) out.push({ key: 'estimate', value: last.estimate.value, text: `≈ ${props.amount(last.estimate.value, true)}`, tone: 'text-accent-text' })
  else if (last?.actual !== null && last?.actual !== undefined) out.push({ key: 'actual', value: last.actual, text: props.amount(last.actual, true), tone: 'text-accent-text' })
  if (props.previousLabel && last?.previous !== null && last?.previous !== undefined)
    out.push({ key: 'previous', value: last.previous, text: props.amount(last.previous, true), tone: 'text-muted' })
  return out
})

const wallClock = (local: string) => Date.parse(`${local}:00Z`)
const formatLocal = (local: string, options: Intl.DateTimeFormatOptions) =>
  new Intl.DateTimeFormat(locale.value, { ...options, timeZone: 'UTC' }).format(wallClock(local))
const bucketLabel = (local: string) =>
  formatLocal(
    local,
    props.granularity === 'hour' ? { hour: '2-digit', minute: '2-digit' } : props.granularity === 'month' ? { month: 'short', year: 'numeric' } : { day: 'numeric', month: 'short' }
  )
/** About one axis label per 72px, at most six. */
const axis = useTemplateRef('axis')
const { width: axisWidth } = useElementSize(axis)
const labelEvery = computed(() => Math.max(1, Math.ceil(points.value.length / Math.max(2, Math.min(6, Math.floor(axisWidth.value / 72))))))

const marks = computed(() =>
  props.annotations.flatMap((annotation) => {
    const index = bucketOf(points.value, annotation.at, props.end)
    return index < 0 ? [] : [{ annotation, index }]
  })
)

const hovered = ref<number | null>(null)
const onMove = (event: PointerEvent) => {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  hovered.value = Math.min(points.value.length - 1, Math.max(0, Math.floor(((event.clientX - box.left) / box.width) * points.value.length)))
}
const tip = computed(() => {
  const index = hovered.value
  const point = index === null ? undefined : points.value[index]
  if (index === null || !point) return null
  return {
    x: x(index),
    title: bucketLabel(point.local),
    actual: point.actual === null ? null : props.amount(point.actual),
    estimate: point.estimate
      ? t('explore.chart.estimate', { value: props.amount(point.estimate.value), low: props.amount(point.estimate.low), high: props.amount(point.estimate.high) })
      : null,
    previous: props.previousLabel && point.previous !== null ? props.amount(point.previous) : null,
  }
})
</script>

<template>
  <figure
    class="m-0"
    :aria-label="label"
  >
    <div
      class="grid grid-cols-[auto_1fr_auto] gap-x-2"
      aria-hidden="true"
    >
      <div class="relative h-56 text-right text-[0.8125rem] text-muted tabular-nums">
        <span class="invisible block h-0 overflow-hidden">{{ ticks[0]?.text }}</span>
        <span
          v-for="tick in ticks"
          :key="tick.value"
          class="absolute right-0 block translate-y-1/2 leading-none"
          :style="{ bottom: `${level(tick.value)}%` }"
        >
          {{ tick.text }}
        </span>
      </div>
      <div
        class="relative h-56"
        @pointermove="onMove"
        @pointerleave="hovered = null"
      >
        <span
          v-for="tick in ticks"
          :key="tick.value"
          class="absolute inset-x-0 border-t border-series-grid"
          :style="{ bottom: `${level(tick.value)}%` }"
        />
        <span
          v-for="mark in marks"
          :key="mark.annotation.id"
          class="absolute inset-y-0 border-l border-dashed border-border-strong"
          :style="{ left: `${((mark.index + 0.5) / count) * 100}%` }"
        />
        <svg
          class="pointer-events-none absolute inset-0 size-full overflow-visible"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
        >
          <path
            v-if="bandPath"
            :d="bandPath"
            class="fill-series-projected-fill stroke-none"
          />
          <path
            v-if="previousPath"
            :d="previousPath"
            fill="none"
            stroke-width="1.5"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-baseline"
          />
          <path
            v-if="estimatePath"
            :d="estimatePath"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-projected [stroke-dasharray:4_3]"
          />
          <path
            :d="actualPath"
            fill="none"
            stroke-width="2.25"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-primary"
          />
        </svg>
        <span
          v-if="current >= 0 && estimatePath"
          class="absolute size-2 -translate-x-1/2 translate-y-1/2 rounded-full bg-series-primary"
          :style="{ left: `${x(current)}%`, bottom: `${level(pace.actual)}%` }"
        />
        <span
          v-for="mark in marks"
          :key="mark.annotation.id"
          class="absolute -top-1 size-2 -translate-x-1/2 rotate-45 bg-accent"
          :style="{ left: `${((mark.index + 0.5) / count) * 100}%` }"
        />
        <template v-if="tip">
          <span
            class="absolute inset-y-0 border-l border-dashed border-border-strong"
            :style="{ left: `${tip.x}%` }"
          />
          <div
            class="pointer-events-none absolute top-2 z-10 min-w-44 rounded-lg border border-popover-border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-soft-lg"
            :style="{ left: `${tip.x}%`, transform: tip.x > 60 ? 'translateX(calc(-100% - 0.75rem))' : 'translateX(0.75rem)' }"
          >
            <p class="mb-1.5 font-medium text-primary">{{ tip.title }}</p>
            <p
              v-if="tip.actual"
              class="flex items-center gap-2"
            >
              <span class="h-0.5 w-2.5 rounded-full bg-series-primary" />
              <span class="ml-auto pl-4 font-medium text-primary tabular-nums">{{ tip.actual }}</span>
            </p>
            <p
              v-if="tip.estimate"
              class="flex items-center gap-2"
            >
              <span class="size-2 shrink-0 rounded-xs border border-dashed border-series-projected bg-series-projected-fill" />
              <span class="font-medium text-primary tabular-nums">{{ tip.estimate }}</span>
            </p>
            <p
              v-if="tip.previous"
              class="flex items-center gap-2"
            >
              <span class="h-0.5 w-2.5 rounded-full bg-series-baseline" />
              <span class="text-muted">{{ previousLabel }}</span>
              <span class="ml-auto pl-4 font-medium text-primary tabular-nums">{{ tip.previous }}</span>
            </p>
          </div>
        </template>
      </div>
      <div class="relative h-56 w-16 text-[0.8125rem] tabular-nums">
        <span
          v-for="end in ends"
          :key="end.key"
          class="absolute left-0 block translate-y-1/2 leading-none whitespace-nowrap"
          :class="end.tone"
          :style="{ bottom: `${level(end.value)}%` }"
        >
          {{ end.text }}
        </span>
      </div>
      <span />
      <div
        ref="axis"
        class="relative mt-2 h-4 text-[0.8125rem] text-muted tabular-nums"
      >
        <span
          v-for="(point, index) in points"
          v-show="index % labelEvery === 0"
          :key="point.start"
          class="absolute -translate-x-1/2 whitespace-nowrap"
          :style="{ left: `${((index + 0.5) / count) * 100}%` }"
        >
          {{ bucketLabel(point.local) }}
        </span>
      </div>
    </div>
    <ul
      class="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[0.875rem] text-muted"
      aria-hidden="true"
    >
      <li class="flex items-center gap-1.5">
        <span class="h-0.5 w-3.5 rounded-full bg-series-primary" />
        {{ t('revenue.summary.pace.actual') }}
      </li>
      <li
        v-if="estimatePath"
        class="flex items-center gap-1.5"
      >
        <span class="w-3.5 border-t-2 border-dashed border-series-projected" />
        {{ t('explore.chart.legendEstimate') }}
      </li>
      <li
        v-if="previousLabel"
        class="flex items-center gap-1.5"
      >
        <span class="h-0.5 w-3.5 rounded-full bg-series-baseline" />
        {{ previousLabel }}
      </li>
      <li
        v-for="mark in marks"
        :key="mark.annotation.id"
        class="flex items-center gap-1.5"
      >
        <span class="size-2 rotate-45 bg-accent" />
        <span class="tabular-nums">{{ bucketLabel(points[mark.index]?.local ?? '') }}</span>
        <span class="text-foreground">{{ mark.annotation.text }}</span>
      </li>
    </ul>
    <table class="sr-only">
      <caption>{{ label }}</caption>
      <thead>
        <tr>
          <th scope="col">{{ t('explore.chart.interval') }}</th>
          <th scope="col">{{ t('revenue.summary.pace.actual') }}</th>
          <th
            v-if="previousLabel"
            scope="col"
          >
            {{ previousLabel }}
          </th>
          <th
            v-if="estimatePath"
            scope="col"
          >
            {{ t('explore.chart.legendEstimate') }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="point in points"
          :key="point.start"
        >
          <th scope="row">{{ bucketLabel(point.local) }}</th>
          <td>{{ point.actual === null ? '–' : amount(point.actual) }}</td>
          <td v-if="previousLabel">{{ point.previous === null ? '–' : amount(point.previous) }}</td>
          <td v-if="estimatePath">
            {{
              point.estimate
                ? t('explore.chart.estimate', { value: amount(point.estimate.value), low: amount(point.estimate.low), high: amount(point.estimate.high) })
                : '–'
            }}
          </td>
        </tr>
      </tbody>
    </table>
  </figure>
</template>
