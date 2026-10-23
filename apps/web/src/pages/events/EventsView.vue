<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { DropdownMenuItem, DropdownMenuSeparator } from '~/components/ui/dropdown-menu'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useAnnotations } from '~/composables/useDefinitions'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFilterFields } from '~/composables/useFilterFields'
import type { ExplorationState, PropertyType, Site } from '~/generated'
import { granularities } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { setGranularity } from '~/services/exploration'
import { eventsState, propertyOf, showEvents, showProperties, statsPanel, tabOf, type EventsTab } from '~/services/events'

import PropertiesView from './PropertiesView.vue'

/**
 * Custom events (CAP-TRAFFIC-06): names with counts and the series over the range. The
 * properties view (CAP-TRAFFIC-07) analyses the data sent with one event or all of them.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const { fields, catalog } = useFilterFields(() => props.site.id)

/** The stored types of the analysed property; `null` while the catalog loads or for no property. */
const typesOf = (state: ExplorationState | null): PropertyType[] | null => {
  const field = state && propertyOf(state)
  if (!field) return null
  const entry = catalog.data.value?.properties.find((p) => p.scope === field.scope && p.key === field.key)
  return entry?.types ?? (catalog.data.value ? [] : null)
}

const screen = useExplorationScreen(
  () => props.site.id,
  () => eventsState(props.site.id, props.site.timezone),
  (state) => (state ? statsPanel(state, typesOf(state)) : []),
  // The statistics panel depends on the property's types, so a property waits for the catalog.
  (state) => !propertyOf(state) || catalog.data.value !== undefined
)
const state = screen.state
const result = screen.result
const resolved = computed(() => result.value?.resolved ?? null)
const annotations = useAnnotations(
  () => props.site.id,
  () => (resolved.value ? { start: resolved.value.start, end: resolved.value.end } : null)
)

const tab = computed<EventsTab>(() => (state.value ? tabOf(state.value) : 'events'))
const tabs = computed(() => [
  { value: 'events' as const, label: t('events.tabs.events') },
  { value: 'properties' as const, label: t('events.tabs.properties') },
])
const choose = (next: EventsTab) => {
  const current = state.value
  if (current && next !== tab.value) void screen.commit(next === 'events' ? showEvents(current) : showProperties(current))
}

const labels = computed(() =>
  (state.value?.measures ?? []).map((measure) =>
    (measure === 'visitor_days' || measure === 'visitors') && result.value
      ? format.measureLabel(result.value.visitor_label)
      : format.measureLabel(measure)
  )
)
const empty = computed(() => !!result.value && result.value.main.row_count === 0)
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
    :title="t('events.title')"
    :heading="{ title: t('events.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template
      v-if="state && !screen.refusal.value"
      #context
    >
      <ChoiceGroup
        :model-value="tab"
        :label="t('events.view')"
        :options="tabs"
        @update:model-value="choose"
      />
    </template>
  </ExplorationHeader>

  <template v-if="state && !screen.refusal.value">

    <template v-if="tab === 'events'">
      <MeasureCards
        class="mb-4"
        :measures="state.measures"
        :cut="result?.main ?? null"
        :visitor-label="result?.visitor_label ?? null"
      />
      <Panel
        class="mb-4"
        :title="t('events.chart.title')"
      >
        <TrendChart
          v-if="result?.main.series"
          :series="result.main.series"
          :measures="state.measures.slice(0, 2)"
          :labels="labels.slice(0, 2)"
          :visualization="state.visualization === 'line' ? 'line' : 'bar'"
          :annotations="annotations.list.data.value?.annotations ?? []"
          :intervals="granularities"
          @interval="screen.commit(setGranularity(state, $event))"
        />
        <Skeleton
          v-else
          class="h-56 w-full"
        />
      </Panel>
      <EmptyState
        v-if="empty"
        icon="lucide:zap"
        :title="t('events.empty')"
        :description="t('events.emptyHint')"
      />
      <ResultTable
        v-else-if="result"
        :state="state"
        :cut="result.main"
        :visitor-label="result.visitor_label"
        :split-fields="fields.map((f) => f.field)"
        @commit="screen.commit"
      >
        <template #cell-actions="{ field, cell }">
          <template v-if="field === 'event_name' && cell.t === 'string'">
            <DropdownMenuSeparator />
            <DropdownMenuItem @select="screen.commit(showProperties(state, cell.v))">
              <Icon
                name="lucide:list-tree"
                aria-hidden="true"
              />
              {{ t('events.showProperties', { name: cell.v }) }}
            </DropdownMenuItem>
          </template>
        </template>
      </ResultTable>
      <Skeleton
        v-else
        class="h-64 w-full"
      />
    </template>

    <PropertiesView
      v-else
      :site="site"
      :state="state"
      :result="result"
      :catalog="catalog.data.value ?? null"
      :types="typesOf(state)"
      :labels="labels"
      :annotations="annotations.list.data.value?.annotations ?? []"
      :split-fields="fields.map((f) => f.field)"
      @commit="screen.commit"
    />
  </template>
</template>
