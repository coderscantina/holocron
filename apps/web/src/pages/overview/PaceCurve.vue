<script setup lang="ts">
import { useElementSize } from '@vueuse/core'
import type { RouteLocationRaw } from 'vue-router'

import { useFormat } from '~/composables/useFormat'
import { hourIndexAt, paceAxis, paceCurves, svgPath, valueAt, type DayPace, type PaceHour } from '~/lib/day-pace'
import { useI18n } from '~/plugins/i18n'

/**
 * Today's running total against the typical weekday's over the whole day: the accent line up to
 * now, the usual day as a neutral line, and the projection from now to midnight as a dashed line
 * in a faint band of its range. Each hour is a column: hovering or focusing it shows the totals
 * by then against usual, and an hour that has begun opens in Explore. Arrow keys move between
 * hours, one tab stop for the whole chart.
 */
const props = defineProps<{
  pace: DayPace
  timezone: string
  /** "a usual Sunday", for labels. */
  usual: string
  link: (hour: PaceHour) => RouteLocationRaw
}>()

const { t, locale } = useI18n()
const format = useFormat()

const root = useTemplateRef('root')
const { width, height } = useElementSize(root)

/** Room for the value labels on the left, the end labels on the right and the hours below. */
const pad = computed(() => (width.value < 560 ? { top: 36, right: 12, bottom: 32, left: 44 } : { top: 20, right: 136, bottom: 32, left: 44 }))
const plot = computed(() => ({
  width: Math.max(0, width.value - pad.value.left - pad.value.right),
  height: Math.max(0, height.value - pad.value.top - pad.value.bottom),
}))

const curves = computed(() => paceCurves(props.pace))
const axis = computed(() => paceAxis(props.pace))
const x = (at: number) => pad.value.left + ((at - props.pace.start) / (props.pace.end - props.pace.start)) * plot.value.width
const y = (value: number) => pad.value.top + plot.value.height - (value / axis.value.max) * plot.value.height

const paths = computed(() => ({
  today: svgPath(curves.value.today, x, y),
  area: curves.value.today.length > 1 ? `${svgPath(curves.value.today, x, y)} L${x(props.pace.now).toFixed(1)} ${y(0).toFixed(1)} L${x(props.pace.start).toFixed(1)} ${y(0).toFixed(1)} Z` : '',
  typical: svgPath(curves.value.typical, x, y),
  projection: svgPath(curves.value.projection, x, y),
  band: svgPath(curves.value.band, x, y, true),
}))

const clock = computed(() => new Intl.DateTimeFormat(locale.value, { timeZone: props.timezone, hour: '2-digit', minute: '2-digit' }))
const time = (ms: number) => clock.value.format(ms)
const hourTicks = computed(() => props.pace.hours.filter((hour) => hour.hour % 6 === 0))

/** End labels at the right edge, pushed apart when they would overlap. */
const endLabels = computed(() => {
  if (pad.value.right < 100) return []
  const labels: { key: string; y: number; text: string; value: string; accent: boolean }[] = []
  const projected = curves.value.projection.at(-1)
  if (projected) labels.push({ key: 'projection', y: y(projected.value), text: t('overview.pace.chart.projected'), value: format.integer(projected.value), accent: true })
  if (props.pace.typicalDay !== null) labels.push({ key: 'typical', y: y(props.pace.typicalDay), text: t('overview.pace.chart.usualDay'), value: format.integer(props.pace.typicalDay), accent: false })
  labels.sort((a, b) => a.y - b.y)
  const second = labels[1]
  const first = labels[0]
  if (first && second && second.y - first.y < 36) {
    const middle = (first.y + second.y) / 2
    first.y = middle - 18
    second.y = middle + 18
  }
  return labels
})

const active = ref<number | null>(null)
const focused = ref(false)
const current = computed(() => hourIndexAt(props.pace, props.pace.now))
const hover = computed(() => {
  const index = active.value
  const hour = index === null ? undefined : props.pace.hours[index]
  if (!hour || index === null) return null
  const until = Math.min(hour.end, props.pace.now)
  return {
    index,
    hour,
    begun: hour.count !== null,
    x: x(hour.count !== null ? until : hour.end),
    columnX: x(hour.start),
    columnWidth: x(hour.end) - x(hour.start),
    today: hour.total,
    typical: curves.value.typical.length > 0 ? valueAt(curves.value.typical, hour.count !== null ? until : hour.end) : null,
    until,
  }
})

/** The hover card sits beside the hour's column, on the side with room, clear of the lines below. */
const TIP_WIDTH = 240
const tipLeft = computed(() => {
  const current = hover.value
  if (!current) return 0
  const right = current.columnX + current.columnWidth + 12
  return right + TIP_WIDTH <= width.value ? right : Math.max(0, current.columnX - TIP_WIDTH - 12)
})

const pick = (event: PointerEvent) => {
  const bounds = root.value?.getBoundingClientRect()
  if (!bounds || plot.value.width === 0) return
  const at = props.pace.start + ((event.clientX - bounds.left - pad.value.left) / plot.value.width) * (props.pace.end - props.pace.start)
  active.value = hourIndexAt(props.pace, Math.min(Math.max(at, props.pace.start), props.pace.end - 1))
}

const tabStop = computed(() => (focused.value && active.value !== null ? active.value : current.value))
const steps: Record<string, (index: number, last: number) => number> = {
  ArrowRight: (index) => index + 1,
  ArrowDown: (index) => index + 1,
  ArrowLeft: (index) => index - 1,
  ArrowUp: (index) => index - 1,
  Home: () => 0,
  End: (_, last) => last,
}
const onKey = (event: KeyboardEvent, index: number) => {
  const step = steps[event.key]
  if (!step) return
  event.preventDefault()
  const last = props.pace.hours.length - 1
  const next = Math.min(Math.max(step(index, last), 0), last)
  active.value = next
  void nextTick(() => root.value?.querySelector<HTMLElement>(`[data-index="${next}"]`)?.focus())
}

const hourName = (hour: PaceHour) => t('overview.pace.chart.hour', { start: time(hour.start), end: time(hour.end) })
const columnLabel = (hour: PaceHour) =>
  hour.count === null
    ? t('overview.pace.chart.columnLater', { hour: hourName(hour), usual: format.integer(hour.typical ?? 0) })
    : t('overview.pace.chart.column', {
        hour: hourName(hour),
        count: format.integer(hour.count),
        usual: hour.typical === null ? '–' : format.integer(hour.typical),
        total: format.integer(hour.total ?? 0),
      })
</script>

<template>
  <div
    ref="root"
    class="relative size-full min-h-[22rem] select-none"
    @pointermove="pick"
    @pointerleave="focused || (active = null)"
  >
    <svg
      v-if="width > 0 && height > 0"
      class="absolute inset-0 overflow-visible"
      :width="width"
      :height="height"
      aria-hidden="true"
    >
      <line
        v-for="tick in axis.ticks"
        :key="tick"
        :x1="pad.left"
        :x2="pad.left + plot.width"
        :y1="y(tick)"
        :y2="y(tick)"
        class="stroke-series-grid"
        stroke-width="1"
      />
      <line
        :x1="pad.left"
        :x2="pad.left + plot.width"
        :y1="y(0)"
        :y2="y(0)"
        class="stroke-hairline"
        stroke-width="1"
      />
      <rect
        v-if="hover"
        :x="hover.columnX"
        :y="pad.top"
        :width="Math.max(0, hover.columnWidth)"
        :height="plot.height"
        class="fill-fill"
      />
      <path
        v-if="paths.band"
        :d="paths.band"
        class="fill-accent/10"
      />
      <path
        :d="paths.typical"
        fill="none"
        class="stroke-series-baseline"
        stroke-width="1.5"
        stroke-linejoin="round"
      />
      <path
        :d="paths.area"
        class="fill-accent/8"
      />
      <path
        :d="paths.projection"
        fill="none"
        class="stroke-accent/70"
        stroke-width="2"
        stroke-dasharray="5 5"
        stroke-linecap="round"
      />
      <path
        :d="paths.today"
        fill="none"
        class="stroke-accent"
        stroke-width="2.5"
        stroke-linejoin="round"
        stroke-linecap="round"
      />
      <line
        :x1="x(pace.now)"
        :x2="x(pace.now)"
        :y1="pad.top"
        :y2="y(0)"
        class="stroke-hairline"
        stroke-dasharray="2 3"
      />
      <circle
        :cx="x(pace.now)"
        :cy="y(pace.today)"
        r="5"
        class="fill-accent stroke-card"
        stroke-width="2"
      />
      <template v-if="hover">
        <circle
          v-if="hover.typical !== null"
          :cx="hover.x"
          :cy="y(hover.typical)"
          r="3.5"
          class="fill-card stroke-series-baseline"
          stroke-width="1.5"
        />
        <circle
          v-if="hover.today !== null"
          :cx="hover.x"
          :cy="y(hover.today)"
          r="4"
          class="fill-accent stroke-card"
          stroke-width="1.5"
        />
      </template>
    </svg>

    <div
      class="pointer-events-none absolute inset-0 text-[0.8125rem] text-muted tabular-nums"
      aria-hidden="true"
    >
      <span
        v-for="tick in axis.ticks"
        :key="tick"
        class="absolute left-0 w-9 -translate-y-1/2 text-right"
        :style="{ top: `${y(tick)}px` }"
      >
        {{ format.compact(tick) }}
      </span>
      <span
        v-for="hour in hourTicks"
        :key="hour.start"
        class="absolute bottom-0 -translate-x-1/2"
        :style="{ left: `${x(hour.start)}px` }"
      >
        {{ time(hour.start) }}
      </span>
      <span
        class="absolute -translate-x-1/2 font-medium text-accent-text"
        :style="{ left: `${x(pace.now)}px`, top: '0' }"
      >
        {{ t('overview.pace.chart.now') }}
      </span>
      <span
        v-if="endLabels.length === 0"
        class="absolute top-0 flex gap-3"
        :style="{ left: `${pad.left}px` }"
      >
        <span class="flex items-center gap-1.5"><span class="h-0.5 w-3 rounded-full bg-accent" />{{ t('overview.pace.chart.today') }}</span>
        <span class="flex items-center gap-1.5"><span class="h-0.5 w-3 rounded-full bg-series-baseline" />{{ usual }}</span>
      </span>
      <span
        v-for="label in endLabels"
        :key="label.key"
        class="absolute flex -translate-y-1/2 flex-col leading-tight"
        :style="{ left: `${pad.left + plot.width + 12}px`, top: `${label.y}px` }"
      >
        <span :class="label.accent ? 'text-accent-text' : 'text-muted'">{{ label.text }}</span>
        <span class="font-semibold text-primary">{{ label.value }}</span>
      </span>
    </div>

    <div
      class="absolute"
      :style="{ left: `${pad.left}px`, top: `${pad.top}px`, width: `${plot.width}px`, height: `${plot.height}px` }"
      role="group"
      :aria-label="t('overview.pace.chart.label', { usual })"
    >
      <template
        v-for="(hour, index) in pace.hours"
        :key="hour.start"
      >
        <RouterLink
          v-if="hour.count !== null"
          :data-index="index"
          :to="link(hour)"
          :tabindex="index === tabStop ? 0 : -1"
          :aria-label="columnLabel(hour)"
          class="absolute inset-y-0 rounded-sm focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
          :style="{ left: `${x(hour.start) - pad.left}px`, width: `${x(hour.end) - x(hour.start)}px` }"
          @focus="((focused = true), (active = index))"
          @blur="((focused = false), (active = null))"
          @keydown="onKey($event, index)"
        />
        <div
          v-else
          :data-index="index"
          :tabindex="index === tabStop ? 0 : -1"
          role="img"
          :aria-label="columnLabel(hour)"
          class="absolute inset-y-0 rounded-sm focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
          :style="{ left: `${x(hour.start) - pad.left}px`, width: `${x(hour.end) - x(hour.start)}px` }"
          @focus="((focused = true), (active = index))"
          @blur="((focused = false), (active = null))"
          @keydown="onKey($event, index)"
        />
      </template>
    </div>

    <div
      v-if="hover"
      class="pointer-events-none absolute z-10 w-60 rounded-lg border border-popover-border bg-popover p-3 text-sm text-popover-foreground shadow-md"
      :style="{ left: `${tipLeft}px`, top: `${pad.top + 8}px` }"
      aria-hidden="true"
    >
      <p class="font-semibold text-primary">{{ hourName(hover.hour) }}</p>
      <template v-if="hover.begun">
        <p class="mt-2 flex justify-between gap-3 tabular-nums">
          <span class="flex items-center gap-2">
            <span class="h-0.5 w-3 rounded-full bg-accent" />
            {{ t('overview.pace.chart.byTime', { time: time(hover.until) }) }}
          </span>
          <span class="font-semibold text-primary">{{ format.integer(hover.today ?? 0) }}</span>
        </p>
        <p
          v-if="hover.typical !== null"
          class="mt-1 flex justify-between gap-3 text-muted tabular-nums"
        >
          <span class="flex items-center gap-2">
            <span class="h-0.5 w-3 rounded-full bg-series-baseline" />
            {{ usual }}
          </span>
          <span>{{ format.integer(Math.round(hover.typical)) }}</span>
        </p>
        <p class="mt-2 border-t border-divider pt-2 text-muted tabular-nums">
          {{ t('overview.pace.chart.thisHour', { count: format.integer(hover.hour.count ?? 0), usual: hover.hour.typical === null ? '–' : format.integer(hover.hour.typical) }) }}
        </p>
        <p class="mt-1 text-[0.8125rem] font-medium text-accent-text">{{ t('overview.pace.chart.open') }}</p>
      </template>
      <p
        v-else
        class="mt-2 text-muted"
      >
        {{ t('overview.pace.chart.later', { usual: hover.hour.typical === null ? '–' : format.integer(hover.hour.typical) }) }}
      </p>
    </div>
  </div>
</template>
