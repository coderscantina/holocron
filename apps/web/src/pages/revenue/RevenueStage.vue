<script setup lang="ts">
import CountryFlag from '~/components/CountryFlag.vue'
import Globe, { type GlobeMarker } from '~/components/globe/Globe.vue'
import StageView from '~/components/stage/StageView.vue'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplore } from '~/composables/useExplore'
import type { Comparison, DimensionField, ExplorationState, Money } from '~/generated'
import { PAGE_SIZE_MAX } from '~/generated/limits'
import { busiest, hourly, placesOf, topOf } from '~/lib/revenue-stage'
import { useI18n } from '~/plugins/i18n'
import { addCondition, encodeState } from '~/services/exploration'

/**
 * When and where the money comes in: net revenue in one currency with its change, revenue by
 * local hour with the busiest three hours marked, and the top countries beside a globe with a
 * disc per country sized by its revenue. Hours, countries and pins open Explore narrowed to them.
 */
const props = defineProps<{ state: ExplorationState; currency: string; net: Money | null; compare?: Comparison }>()

const { t, locale } = useI18n()
const router = useRouter()
const format = useExplorationFormat()

/**
 * Net revenue per currency by one more field, over the page's range and filter. The record
 * count rides along: the engine refuses a cut of money measures alone ("table ea does not exist").
 */
const by = (field: DimensionField) =>
  useExplore(() => ({
    ...props.state,
    measures: ['revenue_net', 'revenue_count'],
    dimensions: ['currency', field],
    sort: [{ by: 'revenue_net', dir: 'desc' }],
    compare: 'none',
    visualization: 'table',
    page: { size: PAGE_SIZE_MAX, index: 0 },
  })).query
const cuts = { hour: by('hour'), country: by('country'), city: by('city'), channel: by('channel') }
const cutBy = (field: keyof typeof cuts) => cuts[field].data.value?.main

const asMoney = (units: number): Money => {
  const exponent = props.net?.exponent ?? 2
  return { currency: props.currency, exponent, minor_units: units, amount: (units / 10 ** exponent).toFixed(exponent) }
}
const amount = (units: number) => format.money(asMoney(units))

const hours = computed(() => hourly(cutBy('hour')?.rows ?? [], props.currency))
const maxHour = computed(() => Math.max(1, ...hours.value))
const peak = computed(() => busiest(hours.value))
const inPeak = (hour: number) => {
  const stretch = peak.value
  return stretch ? (hour - stretch.start + 24) % 24 < stretch.hours : false
}
const clock = computed(() => new Intl.DateTimeFormat(locale.value, { hour: 'numeric', minute: '2-digit', timeZone: 'UTC' }))
const hourText = (hour: number) => clock.value.format(Date.UTC(2000, 0, 1, hour % 24))

const places = computed(() => placesOf(cutBy('country')?.rows ?? [], props.currency))
const total = computed(() => places.value.reduce((sum, place) => sum + place.units, 0))
const countryName = (code: string) => format.textLabel('country', code)
const topCity = computed(() => {
  const top = topOf(cutBy('city')?.rows ?? [], props.currency)
  const cell = top?.key[0]
  return cell?.t === 'string' ? cell.v : null
})
const topChannel = computed(() => {
  const top = topOf(cutBy('channel')?.rows ?? [], props.currency)
  const cell = top?.key[0]
  return top && cell ? { label: format.cellLabel('channel', cell), units: top.units } : null
})

/** One plain sentence: the busiest hours and the place most of the money comes from. */
const takeaway = computed(() => {
  const stretch = peak.value
  const where = topCity.value ?? (places.value[0] ? countryName(places.value[0].country) : null)
  if (!stretch) return null
  const from = hourText(stretch.start)
  const until = hourText(stretch.start + stretch.hours)
  return where ? t('revenue.stage.takeaway', { from, until, where }) : t('revenue.stage.takeawayHours', { from, until })
})

/** Explore with this page's state narrowed by one more condition, by city. */
const explore = (condition: Parameters<typeof addCondition>[1]) => {
  const narrowed = addCondition(
    { ...props.state, dimensions: ['currency', 'city'], sort: [{ by: 'revenue_net', dir: 'desc' }], visualization: 'table' },
    condition
  )
  return { name: 'explore', params: { siteId: props.state.site }, query: { s: encodeState(narrowed) } }
}
const byHour = (hour: number) => explore({ field: 'hour', cmp: 'eq', value: { t: 'integer', v: hour } })
const byCountry = (code: string) => explore({ field: 'country', cmp: 'eq', value: { t: 'string', v: code } })

/** Pins for the top earners only, so small amounts do not crowd the map. */
const PINS = 12
const markers = computed<GlobeMarker[]>(() =>
  places.value.slice(0, PINS).map((place) => {
    const share = format.metric({ t: 'ratio', v: place.units / Math.max(1, total.value), numerator: place.units, denominator: total.value })
    return {
      id: `rev-${place.country}`,
      lat: place.lat,
      lng: place.lng,
      country: place.country,
      weight: place.units,
      scale: place.scale,
      label: t('revenue.stage.pin', { place: countryName(place.country), amount: amount(place.units) }),
      title: countryName(place.country),
      detail: t('revenue.stage.pinDetail', { amount: amount(place.units), share }),
    }
  })
)
const openCountry = (marker: GlobeMarker) => void router.push(byCountry(marker.country))
</script>

<template>
  <StageView :label="t('revenue.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ net ? format.money(net) : '–' }}</p>
        <p class="mt-3 text-[0.9375rem] text-muted tabular-nums">
          <template v-if="compare?.relative_change !== undefined">
            {{ t('revenue.stage.change', { change: format.change(compare.relative_change) }) }}
          </template>
          <template v-else>{{ t('revenue.stage.net') }}</template>
        </p>
        <figure class="m-0 mt-6">
          <figcaption class="sr-only">{{ takeaway ?? t('revenue.stage.byHour') }}</figcaption>
          <div class="flex h-16 items-end gap-[3px]">
            <RouterLink
              v-for="(units, hour) in hours"
              :key="hour"
              :to="byHour(hour)"
              class="group flex h-full flex-1 items-end rounded-t-[1px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              :aria-label="t('revenue.stage.hour', { from: hourText(hour), until: hourText(hour + 1), amount: amount(units) })"
              :title="`${hourText(hour)} · ${amount(units)}`"
            >
              <span
                class="block min-h-px w-full rounded-t-[1px] group-hover:opacity-80"
                :class="inPeak(hour) ? 'bg-accent' : units > 0 ? 'bg-series-comparison' : 'bg-hairline'"
                :style="{ height: `${(units / maxHour) * 100}%` }"
              />
            </RouterLink>
          </div>
          <p
            class="mt-2 grid grid-cols-4 text-sm text-muted tabular-nums"
            aria-hidden="true"
          >
            <span
              v-for="hour in [0, 6, 12, 18]"
              :key="hour"
            >
              {{ hourText(hour) }}
            </span>
          </p>
        </figure>
        <p
          v-if="takeaway"
          class="mt-4 text-[0.9375rem] text-primary"
        >
          {{ takeaway }}
        </p>
      </div>

      <h2 class="sr-only">{{ t('revenue.stage.places') }}</h2>
      <ol class="min-h-0 flex-1 overflow-hidden pt-3 lg:mask-b-from-70%">
        <li
          v-for="(place, index) in places"
          :key="place.country"
        >
          <RouterLink
            :to="byCountry(place.country)"
            class="flex h-10 items-center gap-3 rounded-control text-[0.9375rem] hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            <CountryFlag :code="place.country" />
            <span
              class="min-w-0 flex-1 truncate"
              :class="index === 0 ? 'font-medium text-accent-text' : 'text-primary'"
            >
              {{ countryName(place.country) }}
            </span>
            <span class="shrink-0 text-muted tabular-nums">{{ amount(place.units) }}</span>
          </RouterLink>
        </li>
      </ol>
      <p
        v-if="topChannel"
        class="border-t border-hairline pt-3 text-sm text-muted"
      >
        {{ t('revenue.stage.channel', { channel: topChannel.label, amount: amount(topChannel.units) }) }}
      </p>
    </template>

    <div class="mx-auto aspect-square w-full max-w-[40rem] lg:aspect-auto lg:h-full lg:max-w-none">
      <Globe
        :markers="markers"
        :spotlight="null"
        :label="t('revenue.stage.places')"
        @select="openCountry"
      />
    </div>
  </StageView>
</template>
