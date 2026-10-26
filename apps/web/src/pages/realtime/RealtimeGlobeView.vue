<script setup lang="ts">
import { useIntervalFn, useNow } from '@vueuse/core'

import CountryFlag from '~/components/CountryFlag.vue'
import Globe, { type GlobeMarker } from '~/components/globe/Globe.vue'
import StageView from '~/components/stage/StageView.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useLivePing } from '~/composables/useLivePing'
import type { RealtimeSnapshot, RecentEvent } from '~/generated'
import { age, arrivals, pinId, placeOf, type Spotlight } from '~/lib/realtime-globe'
import { useI18n } from '~/plugins/i18n'

/**
 * The realtime globe: the active count over 30 one-minute bars and the latest events on the
 * left, the globe with a pin per place of the active visitors on the right (above below `lg`).
 * Events that arrive while the page is open turn the globe to them and, with sound on, ping.
 */
const props = defineProps<{ data: RealtimeSnapshot }>()

const { t, locale } = useI18n()
const format = useFormat()
const explorationFormat = useExplorationFormat()
const ping = useLivePing()

/** Ages count on the server's clock: the snapshot's `now` against this browser's, kept per read. */
const skew = computed(() => props.data.now - Date.now())
const now = useNow({ scheduler: (update) => useIntervalFn(update, 1000) })
const relative = computed(() => new Intl.RelativeTimeFormat(locale.value, { style: 'narrow', numeric: 'always' }))
const ageText = (at: number) => {
  const elapsed = age(now.value.getTime() + skew.value - at)
  return elapsed.kind === 'now' ? t('realtime.globe.justNow') : relative.value.format(-elapsed.value, elapsed.unit)
}

const max = computed(() => Math.max(1, ...props.data.series.map((bucket) => bucket.pageviews)))
const pageviews = computed(() => props.data.series.reduce((sum, bucket) => sum + bucket.pageviews, 0))
const visitorMinutes = computed(() => props.data.series.reduce((sum, bucket) => sum + bucket.visitors, 0))

const spotlight = shallowRef<Spotlight | null>(null)
let seenUntil = props.data.recent[0]?.at ?? 0
let seq = 0

watch(
  () => props.data,
  (snapshot) => {
    const fresh = arrivals(snapshot.recent, seenUntil)
    const newest = fresh[0]
    if (!newest) return
    seenUntil = newest.at
    ping.play(fresh.map((event) => event.kind).reverse())
    for (const event of fresh) {
      const place = placeOf(event)
      if (!place) continue
      seq += 1
      spotlight.value = { seq, event, place }
      return
    }
  }
)

const countryName = (code: string) => explorationFormat.textLabel('country', code)

/** A flag pin per place of the active visitors, with their count and latest path. */
const markers = computed<GlobeMarker[]>(() =>
  props.data.active_locations.map((location) => {
    const name = location.city ?? countryName(location.country)
    const active = t('realtime.globe.pinActive', { count: location.visitors })
    return {
      ...location,
      id: pinId(location),
      weight: location.visitors,
      label: t('realtime.globe.pin', {
        place: location.city ? `${location.city}, ${countryName(location.country)}` : name,
        count: location.visitors,
        path: location.path ?? '–',
      }),
      title: name,
      detail: location.path ? `${active} · ${location.path}` : active,
      badge: location.visitors > 1 ? format.integer(location.visitors) : undefined,
    }
  })
)
const spotlightPlace = computed(() => (spotlight.value ? { ...spotlight.value.place, seq: spotlight.value.seq, id: pinId(spotlight.value.place) } : null))

/** Rows fade toward the oldest, as the list runs out under the column. */
const FADE_FROM = 12
const fade = (index: number) => (index < FADE_FROM ? 1 : Math.max(0.15, 1 - (index - FADE_FROM + 1) * 0.11))

const what = (event: RecentEvent) => (event.kind === 'pageview' ? event.path : event.event_name) ?? '–'
const client = (event: RecentEvent) =>
  [event.browser && explorationFormat.textLabel('browser', event.browser), event.device && explorationFormat.textLabel('device', event.device)]
    .filter(Boolean)
    .join(' · ')
</script>

<template>
  <StageView :label="t('realtime.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.integer(data.active) }}</p>
        <p class="mt-3 flex items-center gap-2 text-[0.9375rem] text-muted">
          <span
            class="size-2 rounded-full bg-success"
            aria-hidden="true"
          />
          {{ t('realtime.globe.current') }}
        </p>
        <figure class="m-0 mt-6">
          <figcaption class="sr-only">
            {{ t('realtime.summary', { views: pageviews, visitors: visitorMinutes }) }}
          </figcaption>
          <div
            class="flex h-16 items-end gap-[3px]"
            aria-hidden="true"
          >
            <span
              v-for="(bucket, index) in data.series"
              :key="bucket.start"
              class="min-h-px flex-1 rounded-t-[1px]"
              :class="index === data.series.length - 1 ? 'bg-accent' : bucket.pageviews > 0 ? 'bg-series-comparison' : 'bg-hairline'"
              :style="{ height: `${(bucket.pageviews / max) * 100}%` }"
            />
          </div>
          <p
            class="mt-2 flex justify-between gap-2 text-sm text-muted tabular-nums"
            aria-hidden="true"
          >
            <span>{{ t('realtime.minutes') }}</span>
            <span>{{ t('realtime.globe.pageviews', { count: pageviews }) }}</span>
          </p>
        </figure>
      </div>
      <h2 class="sr-only">{{ t('realtime.recent') }}</h2>
      <ol
        v-if="data.recent.length > 0"
        class="min-h-0 flex-1 overflow-hidden pt-3 lg:mask-b-from-70%"
      >
        <li
          v-for="(event, index) in data.recent"
          :key="`${event.at}-${index}`"
          class="flex h-10 items-center gap-3 text-[0.9375rem] transition-opacity"
          :style="{ opacity: fade(index) }"
          :title="client(event) || undefined"
        >
          <span
            class="size-2 shrink-0 rounded-full bg-success"
            aria-hidden="true"
          />
          <CountryFlag
            v-if="event.country"
            :code="event.country"
          />
          <span
            v-else
            class="w-4.5 shrink-0"
            aria-hidden="true"
          />
          <span
            v-if="event.country"
            class="sr-only"
          >
            {{ explorationFormat.textLabel('country', event.country) }}
          </span>
          <span
            class="min-w-0 flex-1 truncate"
            :class="event.kind === 'custom_event' ? 'font-medium text-accent-text' : 'text-primary'"
          >
            {{ what(event) }}
          </span>
          <span
            v-if="client(event)"
            class="sr-only"
          >
            {{ client(event) }}
          </span>
          <span class="shrink-0 text-muted tabular-nums">{{ ageText(event.at) }}</span>
        </li>
      </ol>
      <p
        v-else
        class="pt-6 text-sm text-muted"
      >
        {{ t('realtime.noRecent') }}
      </p>
    </template>

    <div class="mx-auto aspect-square w-full max-w-[40rem] lg:aspect-auto lg:h-full lg:max-w-none">
      <Globe
        :markers="markers"
        :spotlight="spotlightPlace"
        :label="t('realtime.globe.pins')"
      >
        <template
          v-if="spotlight"
          #spotlight
        >
          <CountryFlag :code="spotlight.place.country" />
          <span class="truncate">{{ spotlight.event.city ?? countryName(spotlight.place.country) }}</span>
          <span
            class="truncate font-semibold"
            :class="spotlight.event.kind === 'custom_event' ? 'text-accent-text' : 'text-primary'"
          >
            {{ spotlight.event.kind === 'pageview' ? spotlight.event.path : spotlight.event.event_name }}
          </span>
        </template>
      </Globe>
    </div>
    <template #controls>
      <SimpleTooltip
        :tooltip="ping.enabled.value ? t('realtime.globe.soundOff') : t('realtime.globe.soundOn')"
        as-child
      >
        <Button
          variant="ghost"
          size="icon"
          class="text-muted hover:text-primary"
          :aria-label="t('realtime.globe.sound')"
          :aria-pressed="ping.enabled.value"
          @click="ping.enabled.value = !ping.enabled.value"
        >
          <Icon
            :name="ping.enabled.value ? 'lucide:volume-2' : 'lucide:volume-x'"
            aria-hidden="true"
          />
        </Button>
      </SimpleTooltip>
    </template>
  </StageView>
</template>
