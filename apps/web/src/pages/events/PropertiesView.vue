<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import { EmptyState } from '~/components/ui/empty-state'
import { Label } from '~/components/ui/label'
import { Panel } from '~/components/ui/panel'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '~/components/ui/select'
import { Skeleton } from '~/components/ui/skeleton'
import type {
  Annotation,
  ExplorationState,
  ExploreResult,
  Field,
  FieldCatalog,
  PropertyType,
  Series,
  Site,
} from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { analyse, isNumeric, pivotOf, propertyOf, selectedEvent, selectEvent, setPivot } from '~/services/events'
import { canonical } from '~/services/exploration'

/**
 * Property analysis of the selected event, or of all custom events (CAP-TRAFFIC-07): values
 * with counts, statistics and sum and mean over time for numbers, and a pivot by a second
 * property. Every choice is a step of the URL state.
 */
const props = defineProps<{
  site: Site
  state: ExplorationState
  result: ExploreResult | null
  catalog: FieldCatalog | null
  /** Stored types of the analysed property. */
  types: PropertyType[] | null
  /** Measure labels of the state, with visitor-days named for the range. */
  labels: string[]
  annotations: Annotation[]
  splitFields: Field[]
}>()

const emit = defineEmits<{ commit: [state: ExplorationState] }>()

const { t } = useI18n()
const ids = { event: useId(), property: useId(), pivot: useId() }

const ALL = 'all'
const NONE = 'none'

const names = useQuery({
  queryKey: computed(() => queryKeys.values(props.site.id, canonical('event_name'))),
  queryFn: ({ signal }) => explorationsResource.values(props.site.id, 'event_name', signal),
  staleTime: 5 * 60 * 1000,
})
const event = computed(() => selectedEvent(props.state))
/** The most frequent names, and the selected one even when it is rarer. Keys are JSON text. */
const events = computed(() => {
  const listed = (names.data.value?.values ?? []).flatMap((entry) => (entry.value.t === 'string' ? [entry.value.v] : []))
  const all = event.value !== null && !listed.includes(event.value) ? [event.value, ...listed] : listed
  return new Map(all.map((name) => [JSON.stringify(name), name]))
})
const eventKey = computed(() => (event.value === null ? ALL : JSON.stringify(event.value)))
const onEvent = (value: unknown) => {
  if (value === ALL) emit('commit', selectEvent(props.state, null))
  else if (typeof value === 'string' && events.value.has(value)) emit('commit', selectEvent(props.state, events.value.get(value) ?? null))
}

const keys = computed(() => props.catalog?.properties ?? [])
const fieldsByKey = computed(
  () => new Map(keys.value.map((p) => [canonical({ scope: p.scope, key: p.key }), { field: { scope: p.scope, key: p.key }, types: p.types }]))
)
const groups = computed(() =>
  (['event', 'session'] as const).flatMap((scope) => {
    const entries = keys.value.filter((p) => p.scope === scope)
    const label: MessageKey = scope === 'event' ? 'events.eventProperties' : 'events.sessionProperties'
    return entries.length > 0 ? [{ scope, label, entries }] : []
  })
)

const property = computed(() => propertyOf(props.state))
const propertyKey = computed(() => (property.value ? canonical(property.value) : undefined))
const onProperty = (value: unknown) => {
  const entry = typeof value === 'string' ? fieldsByKey.value.get(value) : undefined
  if (entry) emit('commit', analyse(props.state, entry.field, entry.types))
}

const pivot = computed(() => pivotOf(props.state))
const pivotKey = computed(() => (pivot.value ? canonical(pivot.value) : NONE))
const onPivot = (value: unknown) => {
  if (value === NONE) emit('commit', setPivot(props.state, null))
  else if (typeof value === 'string') {
    const entry = fieldsByKey.value.get(value)
    if (entry) emit('commit', setPivot(props.state, entry.field))
  }
}

const numeric = computed(() => property.value?.scope === 'event' && !!props.types && isNumeric(props.types))
const notes = computed<MessageKey[]>(() => {
  const types = props.types ?? []
  return [
    ...(types.includes('array') ? (['events.arrayNote'] as const) : []),
    ...(types.length > 0 && types.every((type) => type === 'date' || type === 'null') ? (['events.dateNote'] as const) : []),
    ...(property.value?.scope === 'session' ? (['events.sessionNote'] as const) : []),
  ]
})

/** One measure of the main series, so sum and mean each get their own scale. */
const seriesOf = (index: number): Series | null => {
  const source = props.result?.main.series
  if (!source) return null
  return { ...source, points: source.points.map((point) => ({ ...point, values: point.values.slice(index, index + 1) })) }
}
const charts = computed(() => {
  const field = property.value
  if (!numeric.value || !field) return []
  return [
    { index: 1, title: t('events.sumChart', { key: field.key }), series: seriesOf(1) },
    { index: 2, title: t('events.meanChart', { key: field.key }), series: seriesOf(2) },
  ]
})
const empty = computed(() => !!props.result && props.result.main.row_count === 0)
</script>

<template>
  <div class="mb-4 grid gap-3 sm:grid-cols-3">
    <div class="grid min-w-0 gap-1.5">
      <Label :for="ids.event">{{ t('events.event') }}</Label>
      <Select
        :model-value="eventKey"
        @update:model-value="onEvent"
      >
        <SelectTrigger
          :id="ids.event"
          class="w-full min-w-0"
        >
          <SelectValue class="truncate" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="ALL">{{ t('events.allEvents') }}</SelectItem>
          <SelectItem
            v-for="[key, name] in events"
            :key="key"
            :value="key"
          >
            {{ name }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>
    <div class="grid min-w-0 gap-1.5">
      <Label :for="ids.property">{{ t('events.property') }}</Label>
      <Select
        :model-value="propertyKey"
        :disabled="keys.length === 0"
        @update:model-value="onProperty"
      >
        <SelectTrigger
          :id="ids.property"
          class="w-full min-w-0"
        >
          <SelectValue
            class="truncate"
            :placeholder="t('events.chooseProperty')"
          />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup
            v-for="group in groups"
            :key="group.scope"
          >
            <SelectLabel>{{ t(group.label) }}</SelectLabel>
            <SelectItem
              v-for="entry in group.entries"
              :key="canonical({ scope: entry.scope, key: entry.key })"
              :value="canonical({ scope: entry.scope, key: entry.key })"
            >
              {{ entry.key }}
            </SelectItem>
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
    <div class="grid min-w-0 gap-1.5">
      <Label :for="ids.pivot">{{ t('events.pivot') }}</Label>
      <Select
        :model-value="pivotKey"
        :disabled="!property"
        @update:model-value="onPivot"
      >
        <SelectTrigger
          :id="ids.pivot"
          class="w-full min-w-0"
        >
          <SelectValue class="truncate" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem :value="NONE">{{ t('events.noPivot') }}</SelectItem>
          <SelectGroup
            v-for="group in groups"
            :key="group.scope"
          >
            <SelectLabel>{{ t(group.label) }}</SelectLabel>
            <SelectItem
              v-for="entry in group.entries.filter((e) => canonical({ scope: e.scope, key: e.key }) !== propertyKey)"
              :key="canonical({ scope: entry.scope, key: entry.key })"
              :value="canonical({ scope: entry.scope, key: entry.key })"
            >
              {{ entry.key }}
            </SelectItem>
          </SelectGroup>
        </SelectContent>
      </Select>
    </div>
  </div>

  <template v-if="!property">
    <MeasureCards
      class="mb-4"
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
    />
    <Panel
      class="mb-4"
      :title="t('events.chart.title')"
      :meta="event ?? t('events.allEvents')"
    >
      <TrendChart
        v-if="result?.main.series"
        :series="result.main.series"
        :measures="state.measures.slice(0, 2)"
        :labels="labels.slice(0, 2)"
        :visualization="state.visualization === 'line' ? 'line' : 'bar'"
        :annotations="annotations"
      />
      <Skeleton
        v-else
        class="h-56 w-full"
      />
    </Panel>
    <EmptyState
      v-if="catalog && keys.length === 0"
      icon="lucide:braces"
      :title="t('events.noProperties')"
      :description="t('events.noPropertiesHint')"
    />
    <EmptyState
      v-else-if="catalog"
      icon="lucide:list-tree"
      :title="t('events.pickTitle')"
      :description="t('events.pickHint')"
    />
  </template>

  <template v-else>
    <section
      v-if="numeric"
      class="mb-4"
      :aria-label="t('events.stats', { key: property.key })"
    >
      <MeasureCards
        :measures="result?.panels[0]?.cut.measures ?? []"
        :cut="result?.panels[0] ?? null"
        :visitor-label="result?.visitor_label ?? null"
      />
      <div class="mt-4 grid gap-4 lg:grid-cols-2">
        <Panel
          v-for="chart in charts"
          :key="chart.index"
          :title="chart.title"
        >
          <TrendChart
            v-if="chart.series"
            :series="chart.series"
            :measures="state.measures.slice(chart.index, chart.index + 1)"
            :labels="labels.slice(chart.index, chart.index + 1)"
            visualization="bar"
            :annotations="annotations"
          />
          <Skeleton
            v-else
            class="h-56 w-full"
          />
        </Panel>
      </div>
    </section>
    <MeasureCards
      v-else
      class="mb-4"
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
    />

    <div class="mb-3 flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
      <h2 class="font-semibold text-primary">{{ t('events.values', { key: property.key }) }}</h2>
      <p
        v-for="note in notes"
        :key="note"
        class="text-xs text-muted"
      >
        {{ t(note) }}
      </p>
    </div>
    <EmptyState
      v-if="empty"
      icon="lucide:search-x"
      :title="t('events.noValues')"
      :description="t('events.noValuesHint')"
    />
    <ResultTable
      v-else-if="result"
      :state="state"
      :cut="result.main"
      :visitor-label="result.visitor_label"
      :split-fields="splitFields"
      @commit="(next) => emit('commit', next)"
    />
    <Skeleton
      v-else
      class="h-64 w-full"
    />
  </template>
</template>
