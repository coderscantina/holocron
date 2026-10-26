<script setup lang="ts">
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { useIntervalFn, useNow } from '@vueuse/core'

import { queryKeys } from '~/api/query-keys'
import { hasUrl, linksResource, sourcePath, type SourceKind } from '~/api/resources/links'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import RangeMenu from '~/components/exploration/RangeMenu.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import { chartClock } from '~/components/exploration/trend'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Button } from '~/components/ui/button'
import { Card } from '~/components/ui/card'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExploration } from '~/composables/useExploration'
import { useLinks } from '~/composables/useLinks'
import { usePageMeta } from '~/composables/usePageMeta'
import { usePredicateText } from '~/composables/usePredicateText'
import type { ExplorationState, Field, Granularity, Panel } from '~/generated'
import { breakdownPanel } from '~/pages/overview/breakdown'
import BreakdownPanel from '~/pages/overview/BreakdownPanel.vue'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { canonical, clearFilters, defaultState, fitRange, setGranularity } from '~/services/exploration'

/**
 * The report of one link or pixel: hits and visitor-days with the previous period, the series,
 * and where the hits come from. The same measure cards, chart and breakdowns as a site's
 * overview, over a state in the URL whose `site` is the link or pixel. Days are UTC, the days
 * its visitors are counted in (decision 0020).
 */
const props = defineProps<{ kind: SourceKind }>()

const { t } = useI18n()
const route = useRoute()
const format = useExplorationFormat()
const predicate = usePredicateText()
const { message } = useErrorMessage()

const workspace = computed(() => String(route.params.workspaceId))
const sourceId = computed(() => String(route.params.sourceId))
const { links, pixels } = useLinks(workspace)
const list = computed(() => (props.kind === 'link' ? links : pixels))
const source = computed(() => {
  const items = props.kind === 'link' ? links.data.value?.links : pixels.data.value?.pixels
  return items?.find((item) => item.id === sourceId.value) ?? null
})

usePageMeta(() => source.value?.name ?? t(props.kind === 'link' ? 'links.links' : 'links.pixels'))

const defaults = (): ExplorationState => ({
  ...defaultState(sourceId.value, 'UTC'),
  measures: ['events', 'visitor_days'],
  range: { kind: 'relative', unit: 'day', count: 30 },
})
const exploration = useExploration(sourceId, defaults)
const state = exploration.state

interface PanelSpec {
  field: Field
  title: MessageKey
  count: MessageKey
}
const panelSpecs: PanelSpec[] = [
  { field: 'referrer_domain', title: 'overview.panels.referrers.title', count: 'overview.panels.referrers.count' },
  { field: 'country', title: 'overview.panels.countries.title', count: 'overview.panels.countries.count' },
  { field: 'browser', title: 'overview.panels.browsers.title', count: 'overview.panels.browsers.count' },
  { field: 'device', title: 'overview.panels.devices.title', count: 'overview.panels.devices.count' },
]
const panels: Panel[] = panelSpecs.map((spec) => breakdownPanel(spec.field, 6))

const epoch = ref(0)
const query = useQuery({
  queryKey: computed(() =>
    queryKeys.sourceExplore(workspace.value, props.kind, state.value ? canonical(state.value) : '', canonical(panels), epoch.value)
  ),
  queryFn: ({ signal }) => {
    const current = state.value
    if (!current) throw new Error('no state')
    return linksResource.explore(props.kind, current, panels, signal)
  },
  enabled: computed(() => state.value !== null && source.value !== null),
  staleTime: Infinity,
  placeholderData: keepPreviousData,
})
const result = computed(() => query.data.value ?? null)

const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })
const selected = ref(0)
const series = computed(() => {
  const source = result.value?.main.series
  if (!source) return null
  const pick = <T,>(values: T[]) => values.slice(selected.value, selected.value + 1)
  return {
    ...source,
    points: source.points.map((point) => ({ ...point, values: pick(point.values), compare: point.compare && pick(point.compare) })),
    projection: source.projection?.map((point) => ({ ...point, values: pick(point.values) })),
  }
})
const chartMeasure = computed(() => state.value?.measures[selected.value] ?? 'events')
const chartLabel = computed(() =>
  (chartMeasure.value === 'visitor_days' || chartMeasure.value === 'visitors') && result.value
    ? format.measureLabel(result.value.visitor_label)
    : format.measureLabel(chartMeasure.value)
)
const intervals: Granularity[] = ['hour', 'day']
const clock = computed(() => (result.value ? chartClock(now.value.getTime(), result.value) : undefined))
const filterText = computed(() => (state.value?.filter ? predicate.text(state.value.filter) : null))
const address = computed(() => (source.value ? sourcePath(props.kind, source.value.slug) : ''))
const target = computed(() => (source.value && hasUrl(source.value) ? source.value.url : null))

/** Each breakdown's spec with its cut of the result, in request order. */
const breakdowns = computed(() =>
  panelSpecs.map((spec, index) => ({ title: t(spec.title), count: spec.count, field: spec.field, cut: result.value?.panels[index] ?? null }))
)
</script>

<template>
  <div
    v-if="list.isPending.value"
    class="grid gap-4"
    aria-busy="true"
    :aria-label="t('states.loading')"
  >
    <Skeleton class="h-10 w-64" />
    <Skeleton class="h-48 w-full" />
  </div>
  <EmptyState
    v-else-if="!source"
    icon="lucide:link-2-off"
    :title="t(kind === 'link' ? 'links.linkUnavailable' : 'links.pixelUnavailable')"
    :description="list.isError.value ? message(list.error.value) : undefined"
    class="mx-auto max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'workspace-links', params: { workspaceId: workspace } }">{{ t('links.backToList') }}</RouterLink>
    </Button>
  </EmptyState>
  <template v-else>
    <PageHeader
      :title="source.name"
      :description="address"
    >
      <template #actions>
        <RangeMenu
          v-if="state"
          :range="state.range"
          @change="(range) => state && exploration.commit(fitRange(state, range))"
        />
        <Button
          as-child
          variant="ghost"
        >
          <RouterLink :to="{ name: 'workspace-links', params: { workspaceId: workspace } }">
            <Icon name="lucide:arrow-left" />
            {{ t('links.backToList') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <EmptyState
      v-if="exploration.problem.value"
      icon="lucide:triangle-alert"
      :title="t('links.stateUnreadable')"
      class="mx-auto max-w-lg"
    >
      <Button
        variant="outline"
        @click="exploration.reset()"
      >
        {{ t('links.resetView') }}
      </Button>
    </EmptyState>
    <EmptyState
      v-else-if="query.isError.value"
      icon="lucide:triangle-alert"
      tone="error"
      :title="t('links.reportFailed')"
      :description="message(query.error.value)"
      class="mx-auto max-w-lg"
    >
      <Button
        variant="outline"
        @click="epoch += 1"
      >
        {{ t('actions.retry') }}
      </Button>
    </EmptyState>
    <template v-else-if="state">
      <div
        v-if="filterText"
        class="mb-4 flex flex-wrap items-center gap-2 text-sm text-muted"
      >
        <span>{{ t('links.filteredBy', { filter: filterText }) }}</span>
        <Button
          variant="ghost"
          size="sm"
          @click="exploration.commit(clearFilters(state))"
        >
          {{ t('links.clearFilters') }}
        </Button>
      </div>
      <p
        v-if="target"
        class="mb-1 truncate text-sm text-muted"
        :title="target"
      >
        {{ t('links.target', { url: target }) }}
      </p>
      <p class="mb-4 text-xs text-muted">
        {{ t('links.utcNote') }}
        <template v-if="source.lost_hits > 0">{{ t('links.lostHits', { count: source.lost_hits }) }}</template>
      </p>
      <MeasureCards
        :measures="state.measures"
        :cut="result?.main ?? null"
        :visitor-label="result?.visitor_label ?? null"
        :selected="selected"
        @select="(index) => (selected = index)"
      />
      <Card
        variant="panel"
        class="my-6"
      >
        <TrendChart
          v-if="series"
          :series="series"
          :measures="[chartMeasure]"
          :labels="[chartLabel]"
          :visualization="state.visualization === 'line' ? 'line' : 'bar'"
          :annotations="[]"
          :intervals="intervals"
          :clock="clock"
          @interval="exploration.commit(setGranularity(state, $event))"
        />
        <Skeleton
          v-else
          class="h-80 w-full"
        />
      </Card>
      <div class="grid items-start gap-4 lg:grid-cols-2">
        <BreakdownPanel
          v-for="breakdown in breakdowns"
          :key="breakdown.title"
          v-bind="breakdown"
          :state="state"
          :limit="6"
          :explorable="false"
          @commit="exploration.commit"
        />
      </div>
    </template>
  </template>
</template>
