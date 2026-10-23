<script setup lang="ts">
import type { StorageForecast, StorageSample } from '~/generated'
import { dayInstant, useOperatorStatusText } from '~/lib/operator-status'
import { useI18n } from '~/plugins/i18n'

/**
 * Disk use of the daily samples as a line, the forecast as a dashed continuation and the disk
 * reserve as a dotted line where free space would run out, with an equivalent table for
 * assistive technology.
 */
const props = defineProps<{
  samples: StorageSample[]
  forecast: StorageForecast | null
}>()

const { t } = useI18n()
const text = useOperatorStatusText()

const DAY_MS = 86_400_000
type Point = { x: number; y: number }

const measured = computed<Point[]>(() => props.samples.map((sample) => ({ x: sample.taken_at, y: sample.used_bytes })))

/** Where use stops: Holocron may take everything but the reserve. */
const reserveLine = computed(() => {
  const inputs = props.forecast?.inputs
  return inputs ? Math.max(0, inputs.capacity_bytes - inputs.disk_reserve_bytes) : null
})

/** From the forecast's sample to where use levels off or meets the reserve. */
const projected = computed<Point[]>(() => {
  const forecast = props.forecast
  if (!forecast) return []
  const { inputs, outcome } = forecast
  const start = { x: inputs.sampled_at, y: inputs.used_bytes }
  // A date is the start of its UTC day, which can lie before the sample on the same day.
  const at = (day: string) => Math.max(start.x, dayInstant(day))
  switch (outcome.kind) {
    case 'levels_off':
      return [start, { x: at(outcome.on), y: inputs.steady_state_bytes - inputs.disk_reserve_bytes }]
    case 'reaches_reserve':
      return [start, { x: at(outcome.on), y: Math.max(start.y, reserveLine.value ?? 0) }]
    case 'not_enough_history':
      return []
  }
})

const range = computed(() => {
  const points = [...measured.value, ...projected.value]
  const first = Math.min(...points.map((p) => p.x))
  let last = Math.max(...points.map((p) => p.x))
  // A level forecast keeps going flat for a while, so it reads as levelling off.
  if (props.forecast?.outcome.kind === 'levels_off') last += Math.max(DAY_MS, (last - first) / 8)
  const highest = Math.max(1, reserveLine.value ?? 0, ...points.map((p) => p.y))
  const magnitude = 10 ** Math.floor(Math.log10(highest))
  return { first, span: Math.max(DAY_MS, last - first), top: Math.ceil((highest * 1.05) / magnitude) * magnitude }
})

const left = (x: number) => ((x - range.value.first) / range.value.span) * 100
const level = (y: number) => (Math.max(0, y) / range.value.top) * 100
const path = (points: Point[]) =>
  points.map((p, i) => `${i === 0 ? 'M' : 'L'}${left(p.x).toFixed(3)},${(100 - level(p.y)).toFixed(3)}`).join(' ')

const projectedPath = computed(() => {
  const points = projected.value
  const end = points.at(-1)
  if (!end) return ''
  const flat = props.forecast?.outcome.kind === 'levels_off' ? [{ x: range.value.first + range.value.span, y: end.y }] : []
  return path([...points, ...flat])
})

const ticks = computed(() => [range.value.top, range.value.top / 2, 0])
const formatDate = (ms: number) => text.day(new Date(ms).toISOString().slice(0, 10))
/** First and last sample and the forecast's end, each date once and none crowding the one before. */
const axis = computed(() => {
  const marks: { x: number; label: string }[] = []
  for (const point of [measured.value[0], measured.value.at(-1), projected.value.at(-1)]) {
    if (!point) continue
    const label = formatDate(point.x)
    const previous = marks.at(-1)
    if (previous && (previous.label === label || left(point.x) - left(previous.x) < 15)) continue
    marks.push({ x: point.x, label })
  }
  return marks
})

const summary = computed(() => {
  const first = props.samples[0]
  const last = props.samples.at(-1)
  if (!first || !last) return t('admin.status.forecast.noSample')
  return t('admin.status.chart.summary', {
    count: props.samples.length,
    first: formatDate(first.taken_at),
    last: formatDate(last.taken_at),
    used: text.bytes(last.used_bytes),
  })
})
</script>

<template>
  <figure class="m-0">
    <figcaption class="sr-only">{{ summary }}</figcaption>
    <ul
      class="mb-3 flex flex-wrap items-center gap-4 text-sm text-muted"
      aria-hidden="true"
    >
      <li class="flex items-center gap-1.5">
        <span class="h-0.5 w-4 bg-series-primary" />
        {{ t('admin.status.chart.used') }}
      </li>
      <li
        v-if="projected.length > 0"
        class="flex items-center gap-1.5"
      >
        <span class="w-4 border-t-2 border-dashed border-series-primary" />
        {{ t('admin.status.chart.forecast') }}
      </li>
      <li
        v-if="reserveLine !== null"
        class="flex items-center gap-1.5"
      >
        <span class="w-4 border-t-2 border-dotted border-warning" />
        {{ t('admin.status.chart.reserve') }}
      </li>
    </ul>
    <div
      class="grid grid-cols-[auto_1fr] gap-x-2"
      aria-hidden="true"
    >
      <div class="relative h-56 min-w-12 text-right text-[0.8125rem] text-muted tabular-nums">
        <span
          v-for="tick in ticks"
          :key="tick"
          class="absolute right-0 block translate-y-1/2 leading-none whitespace-nowrap"
          :style="{ bottom: `${level(tick)}%` }"
        >
          {{ text.bytes(tick) }}
        </span>
      </div>
      <div class="relative h-56">
        <span
          v-for="tick in ticks"
          :key="tick"
          class="absolute inset-x-0 border-t border-series-grid"
          :style="{ bottom: `${level(tick)}%` }"
        />
        <svg
          class="absolute inset-0 size-full overflow-visible"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
        >
          <line
            v-if="reserveLine !== null"
            x1="0"
            x2="100"
            :y1="100 - level(reserveLine)"
            :y2="100 - level(reserveLine)"
            stroke-width="2"
            vector-effect="non-scaling-stroke"
            class="stroke-warning [stroke-dasharray:1_4]"
          />
          <path
            :d="path(measured)"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-primary"
          />
          <path
            v-if="projectedPath"
            :d="projectedPath"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-primary [stroke-dasharray:5_4]"
          />
        </svg>
        <span
          v-if="measured.length === 1 && measured[0]"
          class="absolute size-2 -translate-x-1/2 translate-y-1/2 rounded-full bg-series-primary"
          :style="{ left: `${left(measured[0].x)}%`, bottom: `${level(measured[0].y)}%` }"
        />
      </div>
      <span />
      <div class="relative mt-2 h-4 text-[0.8125rem] text-muted tabular-nums">
        <span
          v-for="(mark, index) in axis"
          :key="mark.x"
          class="absolute whitespace-nowrap"
          :class="index === 0 ? '' : index === axis.length - 1 ? '-translate-x-full' : '-translate-x-1/2'"
          :style="{ left: `${left(mark.x)}%` }"
        >
          {{ mark.label }}
        </span>
      </div>
    </div>
    <div class="sr-only">
      <table>
        <caption>{{ summary }}</caption>
        <thead>
          <tr>
            <th scope="col">{{ t('admin.status.chart.date') }}</th>
            <th scope="col">{{ t('admin.status.chart.value') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="sample in samples"
            :key="sample.taken_at"
          >
            <th scope="row">{{ formatDate(sample.taken_at) }}</th>
            <td>{{ text.bytes(sample.used_bytes) }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </figure>
</template>
