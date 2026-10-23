<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import VitalTrack from '~/components/performance/VitalTrack.vue'
import VitalsTable from '~/components/performance/VitalsTable.vue'
import { Button } from '~/components/ui/button'
import { Card } from '~/components/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useVitalFormat } from '~/composables/useVitalFormat'
import type { DimensionField, ExplorationState, Panel, Site } from '~/generated'
import { isVital, rating, vitalLabels, vitals, type Rating } from '~/lib/vitals'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { defaultState } from '~/services/exploration'

/**
 * Web vitals as nearest-rank p50, p75 and p95 (0003 Percentiles): one card per metric with its
 * values on the metric's rating scale, and a table per metric and an optional group. Missing
 * metrics are no observations.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const percentile = useVitalFormat()

const byMetric: Pick<Panel, 'measures' | 'dimensions' | 'sort'> = {
  measures: ['p50', 'p75', 'p95'],
  dimensions: ['perf_metric'],
  sort: [{ by: 'perf_metric', dir: 'asc' }],
}
const defaults = (): ExplorationState => ({
  ...defaultState(props.site.id, props.site.timezone),
  ...byMetric,
  filter: {
    kind: 'group',
    op: 'and',
    items: [
      {
        kind: 'cond',
        id: 'pm',
        field: 'perf_metric',
        cmp: 'in',
        value: { t: 'list', v: vitals.map((v) => ({ t: 'string', v })) },
      },
    ],
  },
  range: { kind: 'relative', unit: 'day', count: 30 },
  compare: 'none',
  visualization: 'table',
})
/** With a breakdown, the cards read the per-metric cut of the same snapshot. */
const panels = (current: ExplorationState | null): Panel[] =>
  current && current.dimensions.length > 1 ? [{ ...byMetric, page: { size: 10, index: 0 } }] : []
const screen = useExplorationScreen(() => props.site.id, defaults, panels)
const state = screen.state
const result = screen.result

const metricCut = computed(() => result.value?.panels[0] ?? result.value?.main ?? null)
const cards = computed(() =>
  (metricCut.value?.rows ?? []).flatMap((row) => {
    const key = row.key[0]
    if (key?.t !== 'string' || !isVital(key.v)) return []
    return [{ vital: key.v, values: row.values, rating: rating(key.v, row.values[1]) }]
  })
)
const ratingLabels = {
  good: 'performance.rating.good',
  improve: 'performance.rating.improve',
  poor: 'performance.rating.poor',
} as const satisfies Record<Rating, MessageKey>
const tone: Record<Rating, string> = { good: 'text-success', improve: 'text-warning', poor: 'text-destructive' }

const breakdowns: DimensionField[] = ['device', 'browser', 'os', 'path', 'country']
const breakdown = computed(() => state.value?.dimensions[1] ?? null)
const isBreakdown = (value: unknown): value is DimensionField => breakdowns.some((b) => b === value)
const breakdownText = computed(() =>
  breakdown.value
    ? t('performance.breakdownBy', { field: format.fieldLabel(breakdown.value) })
    : t('performance.byMetric')
)
const onBreakdown = (value: unknown) => {
  const current = state.value
  if (!current) return
  const second = isBreakdown(value) ? [value] : []
  void screen.commit({
    ...current,
    dimensions: ['perf_metric', ...second],
    sort: [{ by: 'perf_metric', dir: 'asc' }, ...second.map((by) => ({ by, dir: 'asc' as const }))],
    page: { ...current.page, index: 0 },
  })
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
    :title="t('performance.title')"
    :heading="{ title: t('performance.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  />

  <template v-if="state && !screen.refusal.value">
    <template v-if="result">
      <EmptyState
        v-if="cards.length === 0"
        icon="lucide:gauge"
        :title="t('performance.empty')"
        :description="t('performance.emptyHint')"
      />
      <template v-else>
        <div class="@container mb-4">
          <section
            :aria-label="t('performance.cards')"
            class="grid grid-cols-2 gap-3 @3xl:grid-cols-6 @6xl:grid-cols-5"
          >
            <Card
              v-for="card in cards"
              :key="card.vital"
              class="flex flex-col p-4 max-sm:px-3 @max-3xl:last:odd:col-span-2 @3xl:col-span-2 @3xl:nth-[n+4]:col-span-3 @6xl:col-span-1 @6xl:nth-[n+4]:col-span-1"
            >
              <p class="line-clamp-2 text-sm font-medium text-balance text-muted @6xl:min-h-10">
                {{ t(vitalLabels[card.vital]) }}
              </p>
              <p
                class="mt-1.5 truncate text-[1.875rem] leading-tight font-bold tracking-[-0.02em] text-primary tabular-nums"
              >
                {{ percentile(card.values[1]) }}
              </p>
              <p
                class="text-sm font-semibold"
                :class="card.rating ? tone[card.rating] : 'text-muted'"
              >
                {{ t('performance.p75') }} · {{ card.rating ? t(ratingLabels[card.rating]) : '–' }}
              </p>
              <div class="mt-auto pt-4">
                <VitalTrack
                  :vital="card.vital"
                  :values="card.values"
                  :rating="card.rating"
                />
                <p class="mt-1.5 flex justify-between gap-2 text-xs whitespace-nowrap text-muted tabular-nums">
                  <span>{{ format.measureLabel('p50') }} {{ percentile(card.values[0]) }}</span>
                  <span>{{ format.measureLabel('p95') }} {{ percentile(card.values[2]) }}</span>
                </p>
              </div>
            </Card>
          </section>
        </div>

        <div class="mb-3 flex flex-wrap items-center gap-2">
          <DropdownMenu>
            <DropdownMenuTrigger as-child>
              <Button
                variant="outline"
                size="sm"
                :aria-label="t('performance.breakdownLabel', { value: breakdownText })"
              >
                <Icon
                  name="lucide:split"
                  aria-hidden="true"
                />
                {{ breakdownText }}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              <DropdownMenuRadioGroup
                :model-value="breakdown ?? 'none'"
                @update:model-value="onBreakdown"
              >
                <DropdownMenuRadioItem value="none">{{ t('performance.byMetric') }}</DropdownMenuRadioItem>
                <DropdownMenuRadioItem
                  v-for="field in breakdowns"
                  :key="field"
                  :value="field"
                >
                  {{ format.fieldLabel(field) }}
                </DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <SimpleTooltip
            :tooltip="t('performance.note')"
            :aria-label="t('performance.note')"
            class="inline-flex size-7 cursor-help items-center justify-center rounded-control text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            <Icon
              name="lucide:info"
              aria-hidden="true"
            />
          </SimpleTooltip>
        </div>
        <VitalsTable
          :cut="result.main"
          :dimensions="state.dimensions"
        />
      </template>
    </template>
    <Skeleton
      v-else
      class="h-64 w-full"
    />
  </template>
</template>
