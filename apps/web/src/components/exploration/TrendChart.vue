<script setup lang="ts">
import { useElementSize } from '@vueuse/core'
import { Translation } from 'vue-i18n'

import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Annotation, Granularity, Measure, Metric, Series } from '~/generated'
import { granularityLabels, weekdayLabels } from '~/lib/exploration-labels'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import type { Baseline } from '~/services/exploration'

import {
  baselineKey,
  bucketText,
  buckets,
  consequences,
  nowMarker,
  numeric,
  type ChartClock,
  type Phrase,
} from './trend'

/**
 * A series of up to two measures as bars or lines, with annotation markers and an equivalent
 * table for assistive technology. Hovering an interval highlights it and shows its exact values.
 * With one measure it also draws what the data carries: projected buckets after now as dashed
 * outlines, the comparison as a baseline line, a Now marker and the sentences the estimate
 * implies. The `actions` slot sits at the end of the header. Buckets are wall-clock times of the state's timezone, so the axis labels read the
 * server's local bucket starts directly.
 */
const props = defineProps<{
  series: Series
  measures: Measure[]
  visualization: 'bar' | 'line'
  annotations: Annotation[]
  /** Labels for the measures, e.g. visitor-days instead of visitors. */
  labels: string[]
  /** Intervals the header's segmented control offers; without them there is no control. */
  intervals?: readonly Granularity[]
  /** Where now is; without it there is no Now marker. */
  clock?: ChartClock
  /** What the series' comparison is against; the baseline line is drawn only with it. */
  baseline?: Baseline | null
}>()

const emit = defineEmits<{ interval: [granularity: Granularity] }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()

const shown = computed(() => props.measures.slice(0, 2).map((_, index) => index))
const points = computed(() => props.series.points)
const single = computed(() => shown.value.length === 1)
const rows = computed(() => buckets(props.series, 0))
const projected = computed(() => single.value && rows.value.some((row) => row.estimate))
const baselined = computed(() => single.value && !!props.baseline && rows.value.some((row) => row.baseline))
const marker = computed(() => (props.clock ? nowMarker(points.value, props.clock) : null))
/** The last bucket with a measured value; the measured line ends there. */
const lastMeasured = computed(() => rows.value.findLastIndex((row) => !row.future))

/** The next round number at or above a positive value: one significant digit. */
const roundUp = (value: number) => {
  const magnitude = 10 ** Math.floor(Math.log10(value))
  return Math.ceil(value / magnitude) * magnitude
}

/**
 * The value range the plot spans. It always includes zero; negative values, such as net
 * revenue on a day of refunds, extend it below the axis instead of falling out of the plot.
 */
const scale = computed(() => {
  const values = points.value.flatMap((p) => shown.value.map((i) => numeric(p.values[i])))
  if (projected.value) values.push(...rows.value.map((row) => row.estimate?.value ?? 0))
  if (baselined.value) values.push(...rows.value.map((row) => numeric(row.baseline)))
  const highest = Math.max(0, ...values)
  const lowest = Math.min(0, ...values)
  const top = highest > 0 ? roundUp(highest) : lowest < 0 ? 0 : 1
  const bottom = lowest < 0 ? -roundUp(-lowest) : 0
  return { top, bottom, span: top - bottom }
})
const count = (value: number) => format.metric({ t: 'count', v: value })
/** The currency of a series of one-currency amounts, so its axis reads as money. */
const currency = computed(() => {
  const amount = points.value
    .map((point) => point.values[0])
    .find((value) => value?.t === 'money' && value.v.length === 1)
  return amount?.t === 'money' ? (amount.v[0]?.currency ?? null) : null
})
const tickText = computed(() => {
  const code = currency.value
  if (!code) return (value: number) => format.metric({ t: 'count', v: value }, true)
  const money = new Intl.NumberFormat(locale.value, {
    style: 'currency',
    currency: code,
    notation: 'compact',
    maximumFractionDigits: 1,
  })
  return (value: number) => money.format(value)
})
const ticks = computed(() => {
  const { top, bottom } = scale.value
  return (bottom < 0 ? [...new Set([top, 0, bottom])] : [top, top / 2, 0]).map((value) => ({
    value,
    text: tickText.value(value),
  }))
})
/** The longest tick label holds the axis column open, since the labels themselves are positioned. */
const widestTick = computed(() => ticks.value.reduce((wide, tick) => (tick.text.length > wide.length ? tick.text : wide), ''))

/** A bucket's wall-clock start as an instant, read as UTC so the local digits stay as they are. */
const wallClock = (local: string) => {
  const [date = '', time = '00:00'] = local.split('T')
  return Date.parse(`${date}T${time}:00Z`)
}

const formatWallClock = (local: string, options: Intl.DateTimeFormatOptions) =>
  new Intl.DateTimeFormat(locale.value, { ...options, timeZone: 'UTC' }).format(wallClock(local))

const clockTime = (local: string) => formatWallClock(local, { hour: '2-digit', minute: '2-digit' })
const dayName = (local: string) => formatWallClock(local, { weekday: 'short', day: 'numeric', month: 'short' })

/** Short axis label of a bucket. */
const bucketLabel = (local: string, granularity: Granularity) =>
  formatWallClock(
    local,
    granularity === 'hour'
      ? { hour: '2-digit', minute: '2-digit' }
      : granularity === 'month'
        ? { month: 'short', year: 'numeric' }
        : { day: 'numeric', month: 'short' }
  )

/** Full name of a bucket for the tooltip. */
const bucketTitle = (local: string, granularity: Granularity) => {
  switch (granularity) {
    case 'hour':
      return formatWallClock(local, { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })
    case 'day':
      return formatWallClock(local, { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' })
    case 'week':
      return t('explore.chart.weekOf', { date: formatWallClock(local, { day: 'numeric', month: 'short', year: 'numeric' }) })
    case 'month':
      return formatWallClock(local, { month: 'long', year: 'numeric' })
  }
}

/** A message with its numbers formatted for the locale. */
const say = (phrase: Phrase) =>
  t(phrase.key, Object.fromEntries(Object.entries(phrase.named).map(([name, value]) => [name, count(value)])))

const baselineLabel = computed(() => {
  const baseline = props.baseline
  if (!baseline) return ''
  return t(baselineKey(baseline), { weekday: baseline.kind === 'weekday' ? t(weekdayLabels[baseline.weekday]) : '' })
})

const markerLabel = computed(() => {
  const current = marker.value
  if (!current) return ''
  const time = clockTime(current.local)
  return current.stale ? t('explore.chart.dataUntil', { time }) : t('explore.chart.now', { time })
})

/** A sentence under the chart: one message, its emphasised placeholder, and the plain ones. */
const sentence = (key: MessageKey, slot: string, emphasis: string, value = '', end = '') => ({ key, slot, emphasis, value, end })

/** The sentences under the chart, each one message with its emphasised value. */
const sentences = computed(() => {
  if (!projected.value) return []
  const granularity = props.series.granularity
  const measure = props.measures[0]
  const visitors = measure === 'visitors' || measure === 'visitor_days'
  const sumDiffers = visitors && props.series.series_sum_differs
  return consequences(rows.value, granularity, { at: marker.value?.at ?? null, sumDiffers }).map(
    (consequence) => {
      if (consequence.kind === 'passes') {
        return granularity === 'hour'
          ? sentence('explore.chart.consequence.passesAround', 'time', clockTime(consequence.at), count(consequence.value))
          : sentence('explore.chart.consequence.passesOn', 'date', dayName(consequence.at), count(consequence.value))
      }
      return granularity === 'hour'
        ? sentence('explore.chart.consequence.busiestHour', 'start', clockTime(consequence.start), '', clockTime(consequence.end))
        : sentence('explore.chart.consequence.busiestDay', 'date', dayName(consequence.start))
    }
  )
})

/** "Exact counts up to now. The estimate comes from the last 8 Tuesdays." */
const exactness = computed(() => {
  const first = rows.value.find((row) => row.estimate)
  if (!projected.value || !first) return ''
  const exact = marker.value?.stale
    ? t('explore.chart.exact.until', { time: clockTime(marker.value.local) })
    : t('explore.chart.exact.now')
  const basis = bucketText(first, props.baseline ?? null).basis
  return basis ? `${exact} ${say(basis)}` : exact
})

const texts = computed(() => rows.value.map((row) => bucketText(row, props.baseline ?? null)))
const estimateCell = (index: number) => {
  const estimate = texts.value[index]?.estimate
  return estimate ? say(estimate) : ''
}

/** Axis labels get about 72px each, so a phone shows fewer of them than a desk; at most 8. */
const axis = useTemplateRef<HTMLElement>('axis')
const { width: axisWidth } = useElementSize(axis)
const labelEvery = computed(() => {
  const fit = axisWidth.value > 0 ? Math.min(8, Math.max(2, Math.floor(axisWidth.value / 72))) : 8
  return Math.max(1, Math.ceil(points.value.length / fit))
})

const marks = computed(() =>
  props.annotations.flatMap((annotation) => {
    const index = points.value.findIndex(
      (point, i) => point.start <= annotation.at && annotation.at < (points.value[i + 1]?.start ?? Infinity)
    )
    return index < 0 ? [] : [{ annotation, index }]
  })
)

/** Horizontal centre of an interval and the vertical position of a value, in percent of the plot. */
const centre = (index: number) => ((index + 0.5) / points.value.length) * 100
const level = (value: number) => ((value - scale.value.bottom) / scale.value.span) * 100
const height = (metric: Metric | undefined) => level(numeric(metric))

/** A bar from the zero line to its value, downwards for a negative one. */
const bar = (value: number) => ({
  bottom: `${level(Math.min(0, value))}%`,
  height: `${(Math.abs(value) / scale.value.span) * 100}%`,
})

/** A line through the values given, with a gap where a value is missing. */
const path = (values: (number | null)[]) =>
  values
    .map((value, index) =>
      value === null ? null : `${index === 0 || values[index - 1] === null ? 'M' : 'L'}${centre(index).toFixed(3)},${(100 - level(value)).toFixed(3)}`
    )
    .filter((segment) => segment !== null)
    .join(' ')

const linePath = (measure: number) =>
  path(points.value.map((point, index) => (index <= lastMeasured.value ? numeric(point.values[measure]) : null)))

const estimatePath = computed(() =>
  projected.value && props.visualization === 'line' ? path(rows.value.map((row) => row.estimate?.value ?? null)) : ''
)
const baselinePath = computed(() =>
  baselined.value ? path(rows.value.map((row) => (row.baseline ? numeric(row.baseline) : null))) : ''
)

/** The first line closed along the axis, for the soft area under it. */
const areaPath = computed(() => {
  const last = lastMeasured.value
  if (last < 0) return ''
  const zero = (100 - level(0)).toFixed(3)
  return `${linePath(0)} L${centre(last).toFixed(3)},${zero} L${centre(0).toFixed(3)},${zero} Z`
})

const hovered = ref<number | null>(null)
const onMove = (event: PointerEvent) => {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect()
  const total = points.value.length
  if (total === 0 || box.width === 0) return
  hovered.value = Math.min(total - 1, Math.max(0, Math.floor(((event.clientX - box.left) / box.width) * total)))
}

const tip = computed(() => {
  const index = hovered.value
  const point = index === null ? undefined : points.value[index]
  const text = index === null ? undefined : texts.value[index]
  if (index === null || !point || !text) return null
  const x = centre(index)
  const width = 100 / points.value.length
  /** Beside the interval, so it never covers the bars; on the right half it sits to the left. */
  const right = x <= 60
  const baseline = rows.value[index]?.baseline
  return {
    index,
    x,
    left: right ? x + width / 2 : x - width / 2,
    shift: right ? '8px' : 'calc(-100% - 8px)',
    title: bucketTitle(point.local, props.series.granularity),
    rows: text.measured
      ? shown.value.flatMap((index) => {
          const measure = props.measures[index]
          if (!measure) return []
          return [{ index, label: props.labels[index] ?? format.measureLabel(measure), value: format.metric(point.values[index]) }]
        })
      : [],
    estimate: projected.value && text.estimate ? say(text.estimate) : null,
    basis: projected.value && text.basis ? say(text.basis) : null,
    baseline: baselined.value && baseline ? format.metric(baseline) : null,
    notes: marks.value.filter((mark) => mark.index === index).map((mark) => mark.annotation),
  }
})

const summary = computed(() => {
  const first = props.measures[0]
  if (!first || points.value.length === 0) return t('explore.chart.empty')
  const peak = points.value.reduce((best, point) => (numeric(point.values[0]) > numeric(best.values[0]) ? point : best))
  return t('explore.chart.summary', {
    measure: props.labels[0] ?? format.measureLabel(first),
    count: points.value.length,
    peak: bucketLabel(peak.local, props.series.granularity),
    value: format.metric(peak.values[0]),
  })
})

const segments = ref<HTMLButtonElement[]>([])
const step = (index: number, delta: number) => {
  const options = props.intervals ?? []
  const next = (index + delta + options.length) % options.length
  const granularity = options[next]
  if (!granularity) return
  emit('interval', granularity)
  void nextTick(() => segments.value[next]?.focus())
}

/** One measure gets a wide bar; two stand side by side in the interval. */
const barWidth = computed(() => (shown.value.length === 1 ? 'w-[60%] max-w-10' : 'w-[34%] max-w-6'))
const seriesClass = ['bg-series-primary', 'bg-series-secondary']
const strokeClass = ['stroke-series-primary', 'stroke-series-secondary']
</script>

<template>
  <figure class="m-0">
    <figcaption class="sr-only">{{ summary }}</figcaption>
    <div class="mb-3 flex flex-wrap items-center gap-x-4 gap-y-2">
      <div
        v-if="intervals && intervals.length > 0"
        role="radiogroup"
        :aria-label="t('explore.granularity.label')"
        :class="segmentedTrack"
      >
        <button
          v-for="(granularity, index) in intervals"
          :key="granularity"
          ref="segments"
          type="button"
          role="radio"
          :aria-checked="series.granularity === granularity"
          :tabindex="series.granularity === granularity || (index === 0 && !intervals.includes(series.granularity)) ? 0 : -1"
          :class="segmentedItem({ size: 'md' })"
          @click="emit('interval', granularity)"
          @keydown.right.prevent="step(index, 1)"
          @keydown.down.prevent="step(index, 1)"
          @keydown.left.prevent="step(index, -1)"
          @keydown.up.prevent="step(index, -1)"
        >
          {{ t(granularityLabels[granularity]) }}
        </button>
      </div>
      <ul
        class="flex flex-wrap items-center gap-x-4 gap-y-1 text-[0.875rem] text-muted"
        :class="intervals && intervals.length > 0 && 'ml-auto'"
        aria-hidden="true"
      >
        <li
          v-for="index in shown"
          :key="index"
          class="flex items-center gap-1.5"
        >
          <span
            class="size-2.5 rounded-xs"
            :class="seriesClass[index]"
          />
          {{ labels[index] }}
        </li>
        <li
          v-if="projected"
          class="flex items-center gap-1.5"
        >
          <span class="size-2.5 rounded-xs border border-dashed border-series-projected bg-series-projected-fill" />
          {{ t('explore.chart.legendEstimate') }}
        </li>
        <li
          v-if="baselined"
          class="flex items-center gap-1.5"
        >
          <span class="h-0.5 w-2.5 rounded-full bg-series-baseline" />
          {{ baselineLabel }}
        </li>
      </ul>
      <slot name="actions" />
    </div>
    <div
      class="grid grid-cols-[auto_1fr] gap-x-2"
      :class="marker && 'pt-5'"
      aria-hidden="true"
    >
      <div class="relative h-64 text-right text-[0.8125rem] text-muted tabular-nums">
        <span class="invisible block h-0 overflow-hidden">{{ widestTick }}</span>
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
        class="relative h-64"
        @pointermove="onMove"
        @pointerleave="hovered = null"
      >
        <span
          v-for="tick in ticks"
          :key="tick.value"
          class="absolute inset-x-0 border-t"
          :class="tick.value === 0 && scale.bottom < 0 ? 'border-border-strong' : 'border-series-grid'"
          :style="{ bottom: `${level(tick.value)}%` }"
        />
        <span
          v-if="tip"
          class="absolute inset-y-0 rounded-sm bg-primary/[0.04] dark:bg-primary/[0.06]"
          :style="{ left: `${(tip.index / points.length) * 100}%`, width: `${100 / points.length}%` }"
        />
        <div
          v-if="visualization === 'bar'"
          class="absolute inset-0 flex px-[0.5%]"
        >
          <div
            v-for="(point, pointIndex) in points"
            :key="point.start"
            class="flex h-full min-w-0 flex-1 justify-center gap-[2px] transition-opacity duration-150"
            :class="tip && tip.index !== pointIndex && 'opacity-55'"
          >
            <span
              v-for="index in shown"
              :key="index"
              class="relative h-full"
              :class="barWidth"
            >
              <span
                v-if="projected && rows[pointIndex]?.estimate"
                class="absolute inset-x-0 rounded-[3px] border-[1.5px] border-dashed border-series-projected bg-series-projected-fill"
                :style="bar(rows[pointIndex]?.estimate?.value ?? 0)"
              />
              <span
                v-if="!rows[pointIndex]?.future"
                class="absolute inset-x-0 rounded-[3px]"
                :class="seriesClass[index]"
                :style="bar(numeric(point.values[index]))"
              />
            </span>
          </div>
        </div>
        <svg
          class="pointer-events-none absolute inset-0 size-full overflow-visible"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
        >
          <template v-if="visualization === 'line'">
            <path
              :d="areaPath"
              class="fill-series-primary/10 stroke-none"
            />
            <path
              v-for="index in shown"
              :key="index"
              :d="linePath(index)"
              fill="none"
              stroke-width="2"
              stroke-linejoin="round"
              vector-effect="non-scaling-stroke"
              :class="[strokeClass[index], index === 1 ? '[stroke-dasharray:4_3]' : '']"
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
          </template>
          <path
            v-if="baselinePath"
            :d="baselinePath"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-baseline"
          />
        </svg>
        <template v-if="visualization === 'line' && tip && tip.rows.length > 0">
          <span
            class="absolute inset-y-0 border-l border-dashed border-border-strong"
            :style="{ left: `${tip.x}%` }"
          />
          <span
            v-for="index in shown"
            :key="index"
            class="absolute size-2.5 -translate-x-1/2 translate-y-1/2 rounded-full ring-2 ring-card"
            :class="seriesClass[index]"
            :style="{ left: `${tip.x}%`, bottom: `${height(points[tip.index]?.values[index])}%` }"
          />
        </template>
        <template v-if="marker">
          <span
            class="pointer-events-none absolute inset-y-0 border-l border-dashed border-muted"
            :style="{ left: `${marker.position * 100}%` }"
          />
          <span
            class="pointer-events-none absolute bottom-full mb-1 text-[0.8125rem] whitespace-nowrap text-foreground tabular-nums"
            :class="marker.position > 0.5 ? '-ml-1.5 -translate-x-full' : 'ml-1.5'"
            :style="{ left: `${marker.position * 100}%` }"
          >
            {{ markerLabel }}
          </span>
        </template>
        <span
          v-for="mark in marks"
          :key="mark.annotation.id"
          class="absolute -top-1 size-2 -translate-x-1/2 rotate-45 bg-accent"
          :style="{ left: `${centre(mark.index)}%` }"
        />
        <div
          v-if="tip"
          class="pointer-events-none absolute top-2 z-10 min-w-40 rounded-lg border border-popover-border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-soft-lg"
          :style="{ left: `${tip.left}%`, transform: `translateX(${tip.shift})` }"
        >
          <p class="mb-1.5 font-medium text-primary">{{ tip.title }}</p>
          <p
            v-for="row in tip.rows"
            :key="row.index"
            class="flex items-center gap-2"
          >
            <span
              class="size-2 shrink-0 rounded-xs"
              :class="seriesClass[row.index]"
            />
            <span class="text-muted">{{ row.label }}</span>
            <span class="ml-auto pl-4 font-medium text-primary tabular-nums">{{ row.value }}</span>
          </p>
          <p
            v-if="tip.estimate"
            class="flex items-center gap-2"
          >
            <span class="size-2 shrink-0 rounded-xs border border-dashed border-series-projected bg-series-projected-fill" />
            <span class="font-medium text-primary tabular-nums">{{ tip.estimate }}</span>
          </p>
          <p
            v-if="tip.basis"
            class="mt-0.5 max-w-64 pl-4 text-muted"
          >
            {{ tip.basis }}
          </p>
          <p
            v-if="tip.baseline"
            class="flex items-center gap-2"
          >
            <span class="h-0.5 w-2 shrink-0 rounded-full bg-series-baseline" />
            <span class="text-muted">{{ baselineLabel }}</span>
            <span class="ml-auto pl-4 font-medium text-primary tabular-nums">{{ tip.baseline }}</span>
          </p>
          <p
            v-for="note in tip.notes"
            :key="note.id"
            class="mt-1.5 flex items-center gap-2 border-t border-border pt-1.5"
          >
            <span class="size-2 shrink-0 rotate-45 bg-accent" />
            <span class="max-w-56 truncate">{{ note.text }}</span>
          </p>
        </div>
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
          :style="{ left: `${((index + 0.5) / points.length) * 100}%` }"
        >
          {{ bucketLabel(point.local, series.granularity) }}
        </span>
      </div>
    </div>
    <ul
      v-if="marks.length > 0"
      class="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[0.875rem] text-muted"
      :aria-label="t('explore.annotations.label')"
    >
      <li
        v-for="mark in marks"
        :key="mark.annotation.id"
        class="flex items-center gap-1.5"
      >
        <span
          class="size-2 rotate-45 bg-accent"
          aria-hidden="true"
        />
        <span class="tabular-nums">{{ bucketLabel(points[mark.index]?.local ?? '', series.granularity) }}</span>
        <span class="text-foreground">{{ mark.annotation.text }}</span>
      </li>
    </ul>
    <div
      v-if="exactness"
      class="mt-3 flex flex-wrap items-baseline gap-x-6 gap-y-1 border-t border-hairline pt-3 text-[0.875rem] text-muted"
    >
      <Translation
        v-for="sentence in sentences"
        :key="sentence.key"
        :keypath="sentence.key"
        scope="global"
        tag="span"
      >
        <template #value>{{ sentence.value }}</template>
        <template #[sentence.slot]>
          <b class="font-semibold text-primary tabular-nums">{{ sentence.emphasis }}</b>
        </template>
        <template #end>
          <b class="font-semibold text-primary tabular-nums">{{ sentence.end }}</b>
        </template>
      </Translation>
      <span :class="sentences.length > 0 && 'ml-auto text-right'">{{ exactness }}</span>
    </div>
    <div class="sr-only">
      <table>
        <caption>{{ [summary, exactness].filter(Boolean).join(' ') }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('explore.chart.interval') }}</th>
            <th
              v-for="(measure, index) in measures"
              :key="index"
              scope="col"
            >
              {{ labels[index] ?? format.measureLabel(measure) }}
            </th>
            <th
              v-if="projected"
              scope="col"
            >
              {{ t('explore.chart.legendEstimate') }}
            </th>
            <th
              v-if="baselined"
              scope="col"
            >
              {{ baselineLabel }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="(point, pointIndex) in points"
            :key="point.start"
          >
            <th scope="row">{{ point.local.replace('T', ' ') }}</th>
            <td
              v-for="(value, index) in point.values"
              :key="index"
            >
              {{ texts[pointIndex]?.measured === false ? '' : format.metric(value) }}
            </td>
            <td v-if="projected">
              {{ estimateCell(pointIndex) }}
            </td>
            <td v-if="baselined">
              {{ format.metric(rows[pointIndex]?.baseline) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
