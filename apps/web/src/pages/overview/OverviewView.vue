<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'
import { useIntervalFn, useNow } from '@vueuse/core'

import { queryKeys } from '~/api/query-keys'
import { summariesResource } from '~/api/resources/summaries'

import AnnotationsControl from '~/components/exploration/AnnotationsControl.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import GoalPace from '~/components/exploration/GoalPace.vue'
import Headline from '~/components/exploration/Headline.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import { chartClock } from '~/components/exploration/trend'
import { listState } from '~/components/findings/actions'
import FindingList from '~/components/findings/FindingList.vue'
import { Card } from '~/components/ui/card'
import { Skeleton } from '~/components/ui/skeleton'
import { useAnnotations } from '~/composables/useDefinitions'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import type { ExplorationState, ExploreResult, Field, Granularity, Group, Measure, Panel, Site } from '~/generated'
import { FINDINGS_MAX } from '~/generated/limits'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { applySegment, baselineOf, canonical, encodeState, overviewState, setGranularity, splitBy } from '~/services/exploration'

import { breakdownPanel } from './breakdown'
import BreakdownPanel from './BreakdownPanel.vue'

/** The map's outlines load with it, outside the overview's own chunk. */

/**
 * The site overview on real data: a headline sentence, five measures with comparison, the
 * traffic series, and page, referrer, country, browser, system and device panels, all computed
 * in one snapshot from the state in the URL. Without a state it opens on today by the hour
 * against a typical weekday. Countries fetch a full page for the map and list their top ten.
 * The measure the headline and chart show is the `m` parameter, beside the state: choosing it
 * is one history entry and no new request.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const format = useExplorationFormat()

interface PanelSpec {
  field: Field
  title: MessageKey
  count: MessageKey
  /** Rows requested; the map takes every country, its table shows the top ten. */
  size: number
}

/** Panels in request order, which is their order in the result. */
const panelNames = ['pages', 'referrers', 'browsers', 'systems', 'devices', 'countries'] as const
type PanelName = (typeof panelNames)[number]
const panelSpecs: Record<PanelName, PanelSpec> = {
  pages: { field: 'path', title: 'overview.panels.pages.title', count: 'overview.panels.pages.count', size: 10 },
  referrers: { field: 'referrer_domain', title: 'overview.panels.referrers.title', count: 'overview.panels.referrers.count', size: 10 },
  browsers: { field: 'browser', title: 'overview.panels.browsers.title', count: 'overview.panels.browsers.count', size: 6 },
  systems: { field: 'os', title: 'overview.panels.systems.title', count: 'overview.panels.systems.count', size: 6 },
  devices: { field: 'device', title: 'overview.panels.devices.title', count: 'overview.panels.devices.count', size: 6 },
  countries: { field: 'country', title: 'overview.panels.countries.title', count: 'overview.panels.countries.count', size: 100 },
}
const panels: Panel[] = panelNames.map((name) => breakdownPanel(panelSpecs[name].field, panelSpecs[name].size))

const screen = useExplorationScreen(
  () => props.site.id,
  () => overviewState(props.site.id, props.site.timezone),
  () => panels
)
const state = screen.state
const result = screen.result
const resolved = computed(() => result.value?.resolved ?? null)
const annotations = useAnnotations(
  () => props.site.id,
  () => (resolved.value ? { start: resolved.value.start, end: resolved.value.end } : null)
)

const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })

/**
 * The headline's state and result as one pair. While a new result loads, the sentence keeps
 * describing the result on screen instead of collapsing into a placeholder and back.
 */
const told = shallowRef<{ state: ExplorationState; result: ExploreResult } | null>(null)
watchEffect(() => {
  if (state.value && result.value && !screen.previous.value) told.value = { state: state.value, result: result.value }
})

const filtered = computed(() => (state.value?.filter?.items.length ?? 0) > 0)
/**
 * The findings `FindingList` keeps live, read from the same cache entry: no second request or
 * stream. They describe all traffic, so a filtered overview's headline goes without them.
 */
const findingsQuery = useQuery({
  queryKey: computed(() => queryKeys.findings(props.site.id, FINDINGS_MAX)),
  queryFn: ({ signal }) => summariesResource.findings(props.site.id, FINDINGS_MAX, signal),
  staleTime: 60_000,
})
const headlineFindings = computed(() => {
  const answer = findingsQuery.data.value
  if (!answer || filtered.value) return undefined
  return {
    list: answer.findings,
    stale: answer.stale || listState(answer.computed_at, now.value.getTime()) !== 'current',
    annotations: annotations.list.data.value?.annotations ?? [],
  }
})

const measureKey = (measure: Measure) => (typeof measure === 'string' ? measure : canonical(measure))
/** Index of the selected measure in the state; the first when `m` names none of them. */
const selected = computed(() => {
  const key = route.query.m
  const index = (state.value?.measures ?? []).findIndex((measure) => measureKey(measure) === key)
  return Math.max(0, index)
})
const select = (index: number) => {
  const measure = state.value?.measures[index]
  if (!measure || index === selected.value) return
  void router.push({ query: { ...route.query, m: measureKey(measure) } })
}

const chartLabels = computed(() =>
  (state.value?.measures ?? []).map((measure) =>
    (measure === 'visitor_days' || measure === 'visitors') && result.value
      ? format.measureLabel(result.value.visitor_label)
      : format.measureLabel(measure)
  )
)
/** The chart draws the selected measure, with its comparison and estimate. */
const chartIndexes = computed(() => [selected.value])
const series = computed(() => {
  const source = result.value?.main.series
  if (!source) return null
  const pick = <T,>(values: T[]) => chartIndexes.value.flatMap((i) => values[i] ?? [])
  const points = source.points.map((point) => ({
    ...point,
    values: pick(point.values),
    compare: point.compare && pick(point.compare),
  }))
  const projection = source.projection?.map((point) => ({ ...point, values: pick(point.values) }))
  return { ...source, points, projection }
})
const intervals: Granularity[] = ['hour', 'day']
const clock = computed(() => (result.value ? chartClock(now.value.getTime(), result.value) : undefined))
const baseline = computed(() => (state.value && resolved.value ? baselineOf(state.value.compare, resolved.value) : null))

const coverage = computed(() => result.value?.main.identity_coverage ?? null)

/** A breakdown panel's spec and its cut of the snapshot. */
/** Browsers, systems and devices share one panel; the switch picks which one it shows. */
const technologies = ['browsers', 'systems', 'devices'] as const
type Technology = (typeof technologies)[number]
const technology = ref<Technology>('browsers')
const technologyViews = computed(() =>
  technologies.map((name) => ({ value: name, label: t(`overview.panels.technologyViews.${name}`) }))
)
const pickTechnology = (view: string) => {
  const next = technologies.find((name) => name === view)
  if (next) technology.value = next
}

const panel = (name: PanelName) => {
  const spec = panelSpecs[name]
  return { title: t(spec.title), count: spec.count, field: spec.field, cut: result.value?.panels[panelNames.indexOf(name)] ?? null }
}

/** A finding's filter joins the overview's own, as one history entry. */
const filterTo = (group: Group) => {
  if (state.value) screen.commit(applySegment(state.value, group))
}

/** A panel's split opens Explore with the same filters and dates, split by that field. */
const explore = (field: Field) => {
  const current = state.value
  if (!current) return
  const next: ExplorationState = splitBy(
    { ...current, measures: ['pageviews', 'visitor_days'], dimensions: [], sort: [{ by: 'visitor_days', dir: 'desc' }] },
    field
  )
  void router.push({ name: 'explore', params: { siteId: props.site.id }, query: { s: encodeState(next) } })
}
</script>

<template>
  <ExplorationHeader
    :site="site"
    :state="state"
    :problem="screen.problem.value"
    :oversize="screen.oversize.value"
    :result="result"
    :fetching="screen.query.isFetching.value"
    :previous="screen.previous.value"
    :refusal="screen.refusal.value"
    :refused="screen.refused.value"
    :notice="screen.notice.value"
    :title="site.name"
    :heading="{ title: t('nav.overview'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template
      v-if="$slots.context"
      #context
    >
      <slot name="context" />
    </template>
  </ExplorationHeader>

  <template v-if="state && !screen.refusal.value">
    <Headline
      v-if="told"
      class="mb-6"
      :state="told.state"
      :result="told.result"
      :measure="selected"
      :now="now.getTime()"
      :findings="headlineFindings"
      @filter="filterTo"
    />
    <Skeleton
      v-else
      class="mb-6 h-9 w-full max-w-xl"
    />
    <MeasureCards
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
      :selected="selected"
      @select="select"
    />

    <Card
      variant="panel"
      class="my-6"
    >
      <TrendChart
        v-if="series"
        :series="series"
        :measures="chartIndexes.flatMap((i) => state?.measures[i] ?? [])"
        :labels="chartIndexes.flatMap((i) => chartLabels[i] ?? [])"
        :visualization="state.visualization === 'line' ? 'line' : 'bar'"
        :annotations="annotations.list.data.value?.annotations ?? []"
        :intervals="intervals"
        :clock="clock"
        :baseline="baseline"
        @interval="screen.commit(setGranularity(state, $event))"
      >
        <template #actions>
          <AnnotationsControl
            v-if="resolved"
            :site="site.id"
            :range="{ start: resolved.start, end: resolved.end }"
            :timezone="resolved.timezone"
            :can-edit="site.capabilities.includes('site.definitions.write')"
          />
        </template>
      </TrendChart>
      <Skeleton
        v-else
        class="h-80 w-full"
      />
    </Card>

    <FindingList
      :site="site"
      :filtered="filtered"
      @filter="filterTo"
    />

    <div class="grid gap-4">
      <div class="grid items-start gap-4 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
        <BreakdownPanel
          v-bind="panel('pages')"
          :state="state"
          @commit="screen.commit"
          @explore="explore"
        />
        <div class="grid min-w-0 gap-4">
          <GoalPace
            :site="site"
            :state="state"
          />
          <BreakdownPanel
            v-bind="panel('referrers')"
            :state="state"
            @commit="screen.commit"
            @explore="explore"
          />
        </div>
      </div>
      <div class="grid items-start gap-4 lg:grid-cols-2">
        <BreakdownPanel
          v-bind="panel('countries')"
          :state="state"
          :limit="6"
          @commit="screen.commit"
          @explore="explore"
        />
        <BreakdownPanel
          v-bind="panel(technology)"
          :state="state"
          :limit="6"
          :views="technologyViews"
          :view="technology"
          :views-label="t('overview.panels.technology')"
          @view="pickTechnology"
          @commit="screen.commit"
          @explore="explore"
        />
      </div>
    </div>

    <p
      v-if="coverage && coverage.with_identity < coverage.eligible_events"
      class="mt-4 text-xs text-muted"
    >
      {{ t('explore.states.coverage', { with: coverage.with_identity, eligible: coverage.eligible_events }) }}
    </p>
  </template>
</template>
