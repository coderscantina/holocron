<script setup lang="ts">
import { useQueries } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import ChangeBadge from '~/components/exploration/ChangeBadge.vue'
import { bucketOf } from '~/components/exploration/trend'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplore } from '~/composables/useExplore'
import type { Annotation, ExplorationState, Metric } from '~/generated'
import { shades } from '~/lib/event-board'
import { useI18n } from '~/plugins/i18n'
import { boardState, eventSeriesState, trafficState } from '~/services/events'
import { canonical } from '~/services/exploration'

/**
 * The top custom events side by side: count and change, reach (share of visitors) and events
 * per visitor, the revenue they carry, and a strip of their counts over the range, each row
 * shaded on its own scale so its rhythm shows whatever its size. Rows line up by bucket, so a
 * day that moves several events at once reads down the board; annotations mark their bucket.
 * Selecting a row opens its properties in Details.
 */
const props = defineProps<{ state: ExplorationState; annotations: Annotation[] }>()
const emit = defineEmits<{ open: [name: string]; details: [] }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()

const board = useExplore(() => boardState(props.state)).query
const traffic = useExplore(() => trafficState(props.state)).query
const cut = computed(() => board.data.value?.main ?? null)

const countOf = (metric: Metric | undefined) => (metric?.t === 'count' ? metric.v : 0)
const names = computed(() => (cut.value?.rows ?? []).flatMap((row) => (row.key[0]?.t === 'string' ? [row.key[0].v] : [])))

/** One series read per listed event, cached like any exploration of the same state. */
const series = useQueries({
  queries: computed(() =>
    names.value.map((name) => {
      const state = eventSeriesState(props.state, name)
      return {
        queryKey: queryKeys.explore(state.site, canonical(state), canonical([]), 0),
        queryFn: ({ signal }: { signal: AbortSignal }) => explorationsResource.explore(state, [], signal),
        staleTime: Infinity,
      }
    })
  ),
})
const pointsOf = (index: number) => series.value[index]?.data?.main.series?.points ?? null
/** The buckets every row shares, from the first series that has arrived. */
const buckets = computed(() => names.value.map((_, index) => pointsOf(index)).find((points) => points !== null) ?? [])

const visitors = computed(() => countOf(traffic.data.value?.main.totals[0]))
const rows = computed(() =>
  (cut.value?.rows ?? []).flatMap((row, index) => {
    const cell = row.key[0]
    if (cell?.t !== 'string') return []
    const events = countOf(row.values[0])
    const eventVisitors = countOf(row.values[1])
    const revenue = row.values[2]
    const counts = pointsOf(index)?.map((point) => countOf(point.values[0])) ?? null
    const peak = counts ? counts.indexOf(Math.max(...counts)) : -1
    return [
      {
        name: cell.v,
        events,
        compare: row.compare?.[0],
        reach: visitors.value > 0 ? ({ t: 'ratio', v: eventVisitors / visitors.value, numerator: eventVisitors, denominator: visitors.value } as const) : null,
        perVisitor: eventVisitors > 0 ? events / eventVisitors : null,
        revenue: revenue?.t === 'money' && revenue.v.length > 0 ? format.metric(revenue) : null,
        counts,
        shades: counts ? shades(counts) : null,
        peak: peak >= 0 && counts ? { local: buckets.value[peak]?.local ?? '', count: counts[peak] ?? 0 } : null,
      },
    ]
  })
)

const rest = computed(() => {
  const main = cut.value
  if (!main || main.row_count <= rows.value.length) return null
  const shown = rows.value.reduce((sum, row) => sum + row.events, 0)
  return { count: main.row_count - rows.value.length, events: Math.max(0, countOf(main.totals[0]) - shown) }
})

const resolved = computed(() => board.data.value?.resolved ?? null)
const marks = computed(() => {
  const end = resolved.value?.end
  if (end === undefined) return []
  return props.annotations.flatMap((annotation) => {
    const index = bucketOf(buckets.value, annotation.at, end)
    return index < 0 ? [] : [{ annotation, index }]
  })
})
const position = (index: number) => `${((index + 0.5) / Math.max(1, buckets.value.length)) * 100}%`

const granularity = computed(() => board.data.value?.main.series?.granularity ?? props.state.granularity)
const bucketLabel = (local: string) =>
  new Intl.DateTimeFormat(locale.value, {
    ...(granularity.value === 'hour' ? { hour: '2-digit', minute: '2-digit' } : granularity.value === 'month' ? { month: 'short' } : { day: 'numeric', month: 'short' }),
    timeZone: 'UTC',
  }).format(Date.parse(`${local}:00Z`))
const ticks = computed(() => {
  const every = Math.max(1, Math.ceil(buckets.value.length / 5))
  return buckets.value.flatMap((point, index) => (index % every === 0 ? [{ index, text: bucketLabel(point.local) }] : []))
})

const count = (value: number) => format.metric({ t: 'count', v: value })
const perVisitorLabel = computed(() =>
  board.data.value?.visitor_label === 'visitors' ? t('events.board.perVisitor') : t('events.board.perVisitorDay')
)
const meta = computed(() => {
  const main = cut.value
  return main ? t('events.board.meta', { shown: rows.value.length, total: main.row_count, range: format.rangeLabel(props.state.range) }) : undefined
})
const grid = 'grid grid-cols-[minmax(0,1fr)_auto_auto] items-center gap-x-4 md:grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)_4.5rem_5.5rem_6.5rem_6.5rem_8.5rem]'
</script>

<template>
  <EmptyState
    v-if="cut && cut.row_count === 0"
    icon="lucide:zap"
    :title="t('events.empty')"
    :description="t('events.emptyHint')"
  />
  <Panel
    v-else
    :title="t('events.board.title')"
    :meta="meta"
    flush
  >
    <template #actions>
      <p
        class="hidden items-center gap-1.5 text-[0.8125rem] text-muted sm:flex"
        aria-hidden="true"
      >
        {{ t('events.board.fewer') }}
        <span class="size-3 rounded-xs bg-accent/15" />
        <span class="size-3 rounded-xs bg-accent/45" />
        <span class="size-3 rounded-xs bg-accent" />
        {{ t('events.board.more') }}
      </p>
    </template>
    <template v-if="cut">
      <div
        :class="grid"
        class="h-16 items-end px-4.5 pb-2.5 text-[0.8125rem] text-muted"
        aria-hidden="true"
      >
        <span>{{ t('events.event') }}</span>
        <div class="relative order-last col-span-3 h-8 md:order-none md:col-span-1">
          <span
            v-for="mark in marks"
            :key="mark.annotation.id"
            class="absolute top-0 max-w-40 -translate-x-1/2 truncate whitespace-nowrap text-foreground"
            :style="{ left: position(mark.index) }"
            :title="mark.annotation.text"
          >
            {{ mark.annotation.text }}
          </span>
          <span
            v-for="tick in ticks"
            :key="tick.index"
            class="absolute bottom-0 -translate-x-1/2 whitespace-nowrap tabular-nums"
            :style="{ left: position(tick.index) }"
          >
            {{ tick.text }}
          </span>
        </div>
        <span class="text-right">{{ format.measureLabel('events') }}</span>
        <span class="text-right">{{ t('events.board.change') }}</span>
        <span class="hidden text-right md:block">{{ t('events.board.reach') }}</span>
        <span class="hidden text-right whitespace-nowrap md:block">{{ perVisitorLabel }}</span>
        <span class="hidden text-right md:block">{{ format.measureLabel('revenue_net') }}</span>
      </div>
      <ol>
        <li
          v-for="row in rows"
          :key="row.name"
          :class="grid"
          class="relative gap-y-2 border-t border-divider px-4.5 py-2.5 text-sm tabular-nums hover:bg-fill/60 md:h-11 md:py-0"
        >
          <button
            type="button"
            class="min-w-0 truncate text-left text-primary after:absolute after:inset-0 focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:-outline-offset-2 focus-visible:after:outline-ring"
            :aria-label="t('events.board.open', { name: row.name })"
            @click="emit('open', row.name)"
          >
            {{ row.name }}
          </button>
          <div class="relative order-last col-span-3 md:order-none md:col-span-1">
            <div
              v-if="row.shades"
              class="flex h-4.5 gap-[2px]"
              aria-hidden="true"
            >
              <span
                v-for="(shade, index) in row.shades"
                :key="index"
                class="min-w-0 flex-1 rounded-xs bg-accent"
                :style="{ opacity: shade }"
                :title="`${bucketLabel(buckets[index]?.local ?? '')} · ${count(row.counts?.[index] ?? 0)}`"
              />
            </div>
            <Skeleton
              v-else
              class="h-4.5 w-full"
            />
            <span
              v-for="mark in marks"
              :key="mark.annotation.id"
              class="pointer-events-none absolute -inset-y-1 border-l border-dashed border-border-strong"
              :style="{ left: position(mark.index) }"
            />
            <span
              v-if="row.peak"
              class="sr-only"
            >
              {{ t('events.board.peak', { when: bucketLabel(row.peak.local), count: count(row.peak.count) }) }}
            </span>
          </div>
          <span class="text-right">{{ count(row.events) }}</span>
          <span class="text-right">
            <ChangeBadge
              v-if="row.compare"
              :comparison="row.compare"
              measure="events"
            />
            <template v-else>–</template>
          </span>
          <span class="hidden items-center justify-end gap-2 md:flex">
            <span
              class="h-1.5 w-8 overflow-hidden rounded-xs bg-fill"
              aria-hidden="true"
            >
              <span
                class="block h-full bg-series-secondary"
                :style="{ width: `${Math.min(1, row.reach?.v ?? 0) * 100}%` }"
              />
            </span>
            <span class="w-12 text-right">{{ row.reach ? format.metric(row.reach) : '–' }}</span>
          </span>
          <span class="hidden text-right md:block">
            {{ row.perVisitor === null ? '–' : row.perVisitor.toLocaleString(locale, { maximumFractionDigits: 2, minimumFractionDigits: 2 }) }}
          </span>
          <span
            class="hidden truncate text-right md:block"
            :class="row.revenue ? 'text-primary' : 'text-muted'"
            :title="row.revenue ?? undefined"
          >
            {{ row.revenue ?? '–' }}
          </span>
        </li>
      </ol>
      <div
        v-if="rest"
        class="flex items-center justify-between gap-4 border-t border-divider px-4.5 py-3.5 text-sm text-muted"
      >
        <span>{{ t('events.board.rest', { count: count(rest.count), events: count(rest.events) }) }}</span>
        <button
          type="button"
          class="rounded-control text-accent-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          @click="emit('details')"
        >
          {{ t('events.board.openDetails') }}
        </button>
      </div>
    </template>
    <div
      v-else
      class="p-4.5"
    >
      <Skeleton class="h-96 w-full" />
    </div>
  </Panel>
</template>
