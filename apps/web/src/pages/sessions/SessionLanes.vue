<script setup lang="ts">
import { useElementSize } from '@vueuse/core'

import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { SessionTimeline, Site, TimelineSession } from '~/generated'
import { valueIcon } from '~/lib/client-looks'
import { layoutTimeline, timeTicks, timelineDomain, zoneOffset, type PlacedSession } from '~/lib/session-timeline'
import { useI18n } from '~/plugins/i18n'

/**
 * The sessions of a window as people over time: one line per session from its first to its last
 * event, a dot per pageview and an accent dot per custom event, packed into lanes so sessions
 * that do not overlap share a row. A line opens the session; a play mark beside it opens its
 * replay. One tab stop for the whole timeline; arrow keys move through sessions in time order.
 */
const props = defineProps<{
  site: Site
  timeline: SessionTimeline
  /** Capture ID by session, for sessions with a replay. */
  replays: Map<number, string>
}>()

const { t, locale } = useI18n()
const format = useFormat()
const explorationFormat = useExplorationFormat()

/** Lane height range: lanes grow to fill the canvas when there are few of them. */
const LANE_MIN = 18
const LANE_MAX = 30
const EVENTS_SHOWN = 6

const root = useTemplateRef('root')
const canvas = useTemplateRef('canvas')
const scroller = useTemplateRef('scroller')
const { width } = useElementSize(canvas)
const { height } = useElementSize(scroller)

const domain = computed(() => timelineDomain(props.timeline))
/** Room on the right for a replay mark after the latest lines. */
const plotWidth = computed(() => Math.max(0, width.value - 20))
const layout = computed(() => layoutTimeline(props.timeline.sessions, domain.value, plotWidth.value))
const lane = computed(() =>
  Math.min(LANE_MAX, Math.max(LANE_MIN, Math.floor((height.value - 56) / Math.max(1, layout.value.lanes))))
)
const x = (at: number) => ((at - domain.value.start) / (domain.value.end - domain.value.start)) * plotWidth.value
/** Round local times along the top, clear of the Now label at the right edge. */
const ticks = computed(() =>
  timeTicks(domain.value, Math.max(2, Math.floor(plotWidth.value / 110)), zoneOffset(domain.value.end, props.site.timezone)).filter(
    (tick) => x(tick) < plotWidth.value - 56
  )
)

const clock = computed(() => new Intl.DateTimeFormat(locale.value, { timeZone: props.site.timezone, hour: '2-digit', minute: '2-digit' }))
const time = (ms: number) => clock.value.format(ms)
const span = (session: TimelineSession) => format.duration(Math.round((session.end - session.start) / 1000))

const describe = (session: TimelineSession) =>
  [
    time(session.start),
    session.country && explorationFormat.textLabel('country', session.country),
    session.device && explorationFormat.textLabel('device', session.device),
    session.end > session.start ? span(session) : null,
    t('reports.sessions.timeline.pageviews', { count: session.pageviews }),
    session.custom_events > 0 && t('reports.sessions.timeline.customEvents', { count: session.custom_events }),
  ]
    .filter(Boolean)
    .join(', ')

/** Time order for the arrow keys: earliest first, as the lanes were packed. */
const order = computed(() => layout.value.placed)
const latest = computed(() => order.value.length - 1)
const active = ref<number | null>(null)
const focused = ref(false)
const tabStop = computed(() => (focused.value && active.value !== null ? active.value : latest.value))

const card = shallowRef<{ placed: PlacedSession; left: number; top: number } | null>(null)
const show = (index: number, element: HTMLElement) => {
  const placed = order.value[index]
  const bounds = root.value?.getBoundingClientRect()
  if (!placed || !bounds) return
  active.value = index
  const box = element.getBoundingClientRect()
  const cardWidth = 272
  const right = box.right - bounds.left + 12
  const left = right + cardWidth <= bounds.width ? right : Math.max(0, box.left - bounds.left - cardWidth - 12)
  const top = Math.min(Math.max(0, box.top - bounds.top - 16), Math.max(0, bounds.height - 240))
  card.value = { placed, left, top }
}
const hide = () => {
  if (focused.value) return
  active.value = null
  card.value = null
}

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
  const next = Math.min(Math.max(step(index, latest.value), 0), latest.value)
  active.value = next
  void nextTick(() => {
    const element = canvas.value?.querySelector<HTMLElement>(`[data-index="${next}"]`)
    element?.focus()
    element?.scrollIntoView({ block: 'nearest' })
  })
}
const onEnter = (index: number, event: PointerEvent) => {
  if (event.currentTarget instanceof HTMLElement) show(index, event.currentTarget)
}
const onFocus = (index: number, event: FocusEvent) => {
  focused.value = true
  if (event.currentTarget instanceof HTMLElement) show(index, event.currentTarget)
}
const onBlur = () => {
  focused.value = false
  hide()
}

const what = (event: TimelineSession['events'][number]) => (event.kind === 'pageview' ? event.path : event.event_name) ?? '–'
</script>

<template>
  <div
    ref="root"
    class="absolute inset-0 flex flex-col lg:left-6"
  >
    <div
      class="relative h-6 shrink-0 text-[0.8125rem] text-muted tabular-nums"
      aria-hidden="true"
    >
      <span
        v-for="tick in ticks"
        :key="tick"
        class="absolute top-0 -translate-x-1/2 whitespace-nowrap"
        :style="{ left: `${x(tick)}px` }"
      >
        {{ time(tick) }}
      </span>
      <span
        class="absolute top-0 right-5 translate-x-1/2 font-medium text-accent-text"
      >
        {{ t('reports.sessions.timeline.now') }}
      </span>
    </div>
    <div
      ref="scroller"
      class="min-h-0 flex-1 overflow-y-auto overscroll-contain"
    >
      <div
        ref="canvas"
        role="group"
        :aria-label="t('reports.sessions.timeline.label')"
        class="relative min-h-full"
        :style="{ height: `${layout.lanes * lane + 56}px` }"
        @pointerleave="hide"
      >
        <span
          v-for="tick in ticks"
          :key="tick"
          class="absolute inset-y-0 border-l border-series-grid"
          :style="{ left: `${x(tick)}px` }"
          aria-hidden="true"
        />
        <span
          class="absolute inset-y-0 border-l border-dashed border-hairline"
          :style="{ left: `${plotWidth}px` }"
          aria-hidden="true"
        />
        <template
          v-for="(placed, index) in layout.placed"
          :key="placed.session.session"
        >
          <RouterLink
            :to="{ name: 'session', params: { siteId: site.id, sessionId: placed.session.session } }"
            :data-index="index"
            :tabindex="index === tabStop ? 0 : -1"
            :aria-label="t('reports.sessions.timeline.open', { session: describe(placed.session) })"
            class="group absolute rounded-full focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring"
            :style="{ left: `${placed.x}px`, top: `${placed.lane * lane + 4}px`, width: `${placed.width}px`, height: `${lane - 4}px` }"
            @pointerenter="onEnter(index, $event)"
            @focus="onFocus(index, $event)"
            @blur="onBlur"
            @keydown="onKey($event, index)"
          >
            <span
              class="absolute inset-x-0 top-1/2 h-0.5 -translate-y-1/2 rounded-full"
              :class="active === index ? 'bg-accent' : 'bg-series-baseline/60 group-hover:bg-series-baseline'"
            />
            <span
              v-for="(mark, markIndex) in placed.marks"
              :key="markIndex"
              class="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full"
              :class="mark.custom ? 'z-10 size-2 bg-accent ring-2 ring-surface' : active === index ? 'size-1.5 bg-accent' : 'size-1.5 bg-series-baseline'"
              :style="{ left: `${mark.x}px` }"
            />
          </RouterLink>
          <RouterLink
            v-if="replays.has(placed.session.session)"
            :to="{ name: 'replay', params: { siteId: site.id, captureId: replays.get(placed.session.session) } }"
            :tabindex="index === tabStop ? 0 : -1"
            :aria-label="t('reports.sessions.timeline.replay', { session: describe(placed.session) })"
            class="absolute flex size-3.5 items-center justify-center rounded-full text-muted hover:text-accent-text focus-visible:outline-2 focus-visible:outline-ring"
            :style="{ left: `${placed.x + placed.width + 2}px`, top: `${placed.lane * lane + 4}px` }"
          >
            <Icon
              name="lucide:play"
              size="10"
              aria-hidden="true"
            />
          </RouterLink>
        </template>
      </div>
    </div>

    <div
      v-if="card"
      class="pointer-events-none absolute z-20 w-68 rounded-lg border border-popover-border bg-popover p-3 text-sm text-popover-foreground shadow-md"
      :style="{ left: `${card.left}px`, top: `${card.top + 24}px` }"
      aria-hidden="true"
    >
      <p class="flex items-center gap-2 font-semibold text-primary">
        <CountryFlag
          v-if="card.placed.session.country"
          :code="card.placed.session.country"
        />
        <span class="truncate">{{ card.placed.session.country ? explorationFormat.textLabel('country', card.placed.session.country) : t('reports.sessions.timeline.somewhere') }}</span>
        <Icon
          v-if="card.placed.session.device && valueIcon('device', card.placed.session.device)"
          :name="valueIcon('device', card.placed.session.device) ?? ''"
          size="14"
          class="ml-auto shrink-0 text-muted"
        />
      </p>
      <p class="mt-1 text-muted tabular-nums">
        {{ time(card.placed.session.start) }}<template v-if="card.placed.session.end > card.placed.session.start"> · {{ span(card.placed.session) }}</template>
        · {{ t('reports.sessions.timeline.pageviews', { count: card.placed.session.pageviews }) }}
      </p>
      <ol class="mt-2 flex flex-col gap-1 border-t border-divider pt-2">
        <li
          v-for="(event, index) in card.placed.session.events.slice(0, EVENTS_SHOWN)"
          :key="index"
          class="flex items-center gap-2"
        >
          <span
            class="size-1.5 shrink-0 rounded-full"
            :class="event.kind === 'custom_event' ? 'size-2 bg-accent' : 'bg-series-baseline'"
          />
          <span
            class="truncate"
            :class="event.kind === 'custom_event' ? 'font-medium text-accent-text' : 'text-primary'"
          >{{ what(event) }}</span>
        </li>
        <li
          v-if="card.placed.session.events.length > EVENTS_SHOWN || card.placed.session.truncated"
          class="pl-3.5 text-muted"
        >
          {{ t('reports.sessions.timeline.moreEvents') }}
        </li>
      </ol>
      <p
        v-if="replays.has(card.placed.session.session)"
        class="mt-2 flex items-center gap-1.5 text-muted"
      >
        <Icon
          name="lucide:play"
          size="12"
        />
        {{ t('reports.sessions.timeline.hasReplay') }}
      </p>
    </div>
  </div>
</template>
