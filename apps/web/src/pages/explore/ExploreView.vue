<script setup lang="ts">
import DimensionPicker from '~/components/exploration/DimensionPicker.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import MeasurePicker from '~/components/exploration/MeasurePicker.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import WeekGrid from '~/components/exploration/WeekGrid.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Card } from '~/components/ui/card'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useAnnotations } from '~/composables/useDefinitions'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFilterFields } from '~/composables/useFilterFields'
import type { ExplorationState, Panel, Site, Visualization } from '~/generated'
import { granularities, visualizationLabels, type OfferedVisualization } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { defaultState, setGranularity, setVisualization } from '~/services/exploration'

/** The full exploration: measures, dimensions, visualization, chart and table of one state. */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()

const defaults = (): ExplorationState => ({
  ...defaultState(props.site.id, props.site.timezone),
  measures: ['pageviews', 'visitor_days'],
  dimensions: ['path'],
  sort: [{ by: 'visitor_days', dir: 'desc' }],
  visualization: 'table',
})

/** The week grid has up to 168 cells, so it reads two full pages in the same snapshot. */
const panels = (current: ExplorationState | null): Panel[] => {
  const measure = current?.measures[0]
  if (current?.visualization !== 'grid' || !measure) return []
  return [0, 1].map((index) => ({
    measures: [measure],
    dimensions: ['weekday', 'hour'],
    sort: [],
    page: { size: 100, index },
  }))
}
const screen = useExplorationScreen(() => props.site.id, defaults, panels)
const state = screen.state
const result = screen.result
const { fields } = useFilterFields(() => props.site.id)

const resolved = computed(() => result.value?.resolved ?? null)
const annotations = useAnnotations(
  () => props.site.id,
  () => (resolved.value ? { start: resolved.value.start, end: resolved.value.end } : null)
)

const labels = computed(() =>
  (state.value?.measures ?? []).map((measure) =>
    (measure === 'visitor_days' || measure === 'visitors') && result.value
      ? format.measureLabel(result.value.visitor_label)
      : format.measureLabel(measure)
  )
)
const series = computed(() => state.value?.visualization === 'line' || state.value?.visualization === 'bar')
const visualizationOptions = computed(() =>
  (['table', 'bar', 'line', 'grid'] as const satisfies readonly OfferedVisualization[]).map((value) => ({
    value,
    label: t(visualizationLabels[value]),
  }))
)

const choose = (visualization: Visualization) => {
  const current = state.value
  if (!current) return
  if (visualization === 'grid') {
    screen.commit({ ...current, visualization, dimensions: ['weekday', 'hour'], sort: [], page: { ...current.page, index: 0 } })
    return
  }
  screen.commit(setVisualization(current, visualization))
}

const gridRows = computed(() => (result.value?.panels ?? []).flatMap((cut) => cut.rows))
const empty = computed(() => {
  const main = result.value?.main
  return !!main && main.identity_coverage.eligible_events === 0 && main.rows.length === 0
})
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
    :title="t('explore.title')"
    :heading="{ title: t('explore.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  />

  <template v-if="state && !screen.refusal.value">
    <div class="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2">
      <MeasurePicker
        :state="state"
        :fields="fields"
        @commit="screen.commit"
      />
      <ChoiceGroup
        :model-value="state.visualization"
        :label="t('explore.visualization.label')"
        :options="visualizationOptions"
        @update:model-value="choose"
      />
      <DimensionPicker
        v-if="state.visualization !== 'grid'"
        :state="state"
        :fields="fields"
        @commit="screen.commit"
      />
    </div>

    <MeasureCards
      class="mb-4"
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
    />

    <EmptyState
      v-if="empty"
      icon="lucide:search-x"
      :title="t('explore.states.empty')"
      :description="t('explore.states.emptyHint')"
      class="mb-4"
    />

    <Card
      v-if="series || state.visualization === 'grid'"
      class="mb-4"
    >
      <TrendChart
        v-if="series && result?.main.series"
        :series="result.main.series"
        :measures="state.measures.slice(0, 2)"
        :labels="labels.slice(0, 2)"
        :visualization="state.visualization === 'line' ? 'line' : 'bar'"
        :annotations="annotations.list.data.value?.annotations ?? []"
        :intervals="granularities"
        @interval="screen.commit(setGranularity(state, $event))"
      />
      <WeekGrid
        v-else-if="state.visualization === 'grid' && result && state.measures[0]"
        :rows="gridRows"
        :measure="state.measures[0]"
        :label="labels[0] ?? ''"
      />
      <Skeleton
        v-else
        class="h-56 w-full"
      />
    </Card>

    <template v-if="state.dimensions.length > 0 && state.visualization !== 'grid'">
      <ResultTable
        v-if="result"
        :state="state"
        :cut="result.main"
        :visitor-label="result.visitor_label"
        :split-fields="fields.map((f) => f.field)"
        @commit="screen.commit"
      />
      <Skeleton
        v-else
        class="h-64 w-full"
      />
    </template>
  </template>
</template>
