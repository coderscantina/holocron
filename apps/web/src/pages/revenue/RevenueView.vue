<script setup lang="ts">
import { useIntervalFn, useNow } from '@vueuse/core'
import { toast } from 'vue-sonner'

import Icon from '~/components/Icon.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import { chartClock } from '~/components/exploration/trend'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
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
import { useAnnotations } from '~/composables/useDefinitions'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFormat } from '~/composables/useFormat'
import type { Cut, DimensionField, ExplorationState, Measure, Metric, Panel, Row, Series, Site } from '~/generated'
import { PAGE_SIZE_MAX } from '~/generated/limits'
import { granularities } from '~/lib/exploration-labels'
import { useViewMode } from '~/composables/useViewMode'
import { useI18n } from '~/plugins/i18n'
import { baselineOf, defaultState, setGranularity } from '~/services/exploration'

import RevenueSummary from './RevenueSummary.vue'

/**
 * Revenue one currency at a time (0003 Revenue): net, gross, refunds and records with their
 * change against the previous period, the measure picked in the strip over time against the
 * previous period, and a breakdown that keeps currencies apart. Nothing is converted: with
 * several currencies, a switch beside the title picks the one the strip and chart show. The
 * chart's measure is the `m` parameter, beside the state, as on the overview. Two views of the
 * same state: the summary of pace and paying sources (default) and these details; the choice is
 * remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const format = useExplorationFormat()
const numbers = useFormat()
const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })

const byCurrency: Pick<Panel, 'measures' | 'dimensions' | 'sort'> = {
  measures: ['revenue_net', 'revenue_gross', 'revenue_refunds', 'revenue_count'],
  dimensions: ['currency'],
  sort: [{ by: 'currency', dir: 'asc' }],
}
const defaults = (): ExplorationState => ({
  ...defaultState(props.site.id, props.site.timezone),
  ...byCurrency,
  dimensions: ['currency', 'channel'],
  sort: [{ by: 'revenue_net', dir: 'desc' }],
  range: { kind: 'relative', unit: 'day', count: 30 },
  compare: 'previous_period',
  visualization: 'bar',
  page: { size: PAGE_SIZE_MAX, index: 0 },
})
const view = useViewMode('revenue', ['summary', 'details'])
const views = computed(() => [
  { value: 'summary' as const, label: t('revenue.view.summary') },
  { value: 'details' as const, label: t('revenue.view.details') },
])
/** With a breakdown, the strip reads the per-currency cut of the same snapshot. */
const panels = (current: ExplorationState | null): Panel[] =>
  current && current.dimensions.length > 1 ? [{ ...byCurrency, page: { size: 50, index: 0 } }] : []
const screen = useExplorationScreen(() => props.site.id, defaults, panels)
const state = screen.state
const result = screen.result

const resolved = computed(() => result.value?.resolved ?? null)
const annotations = useAnnotations(
  () => props.site.id,
  () => (resolved.value ? { start: resolved.value.start, end: resolved.value.end } : null)
)

const indexOf = (measure: Measure) => state.value?.measures.indexOf(measure) ?? -1
/** A metric narrowed to one currency; counts and other metrics stay as they are. */
const only = (metric: Metric, code: string): Metric =>
  metric.t === 'money' ? { t: 'money', v: metric.v.filter((money) => money.currency === code) } : metric
const codeOf = (row: Row) => (row.key[0]?.t === 'string' ? row.key[0].v : null)

/** Currencies with revenue in range, in the order of the totals. */
const currencies = computed(() => {
  const net = result.value?.main.totals[indexOf('revenue_net')]
  return net?.t === 'money' ? net.v.map((money) => money.currency) : []
})
const chosen = ref<string | null>(null)
const currency = computed(() =>
  chosen.value && currencies.value.includes(chosen.value) ? chosen.value : (currencies.value[0] ?? null)
)

/** The strip's cut: the chosen currency's row with its change against the previous period. */
const strip = computed<Cut | null>(() => {
  const cut = result.value?.panels[0] ?? result.value?.main
  const code = currency.value
  if (!cut || !code) return null
  const row = cut.rows.find((candidate) => codeOf(candidate) === code)
  const values = row?.values ?? result.value?.main.totals ?? []
  return { ...cut, totals: values.map((value) => only(value, code)), compare: row?.compare, rows: [] }
})

/** The chosen currency's amount of a measure in minor units, with its currency for formatting. */
const amount = (measure: Measure) => {
  const metric = strip.value?.totals[indexOf(measure)]
  const money = metric?.t === 'money' ? metric.v[0] : undefined
  return money ? { money, units: Number(money.minor_units) } : null
}
/** The chosen currency's revenue records. */
const stripCount = computed(() => {
  const count = strip.value?.totals[indexOf('revenue_count')]
  return count?.t === 'count' ? count.v : 0
})
/** Refund rate and net per record, derived from the strip's own amounts. */
const facts = computed(() => {
  const gross = amount('revenue_gross')
  const net = amount('revenue_net')
  const count = strip.value?.totals[indexOf('revenue_count')]
  const records = count?.t === 'count' ? count.v : 0
  const refunds = Math.abs(amount('revenue_refunds')?.units ?? 0)
  const parts: string[] = []
  if (gross && gross.units > 0) parts.push(t('revenue.refundRate', { rate: numbers.percent(refunds / gross.units) }))
  if (net && records > 0) {
    const units = Math.round(net.units / records)
    const { exponent } = net.money
    const perRecord = { ...net.money, minor_units: units, amount: (units / 10 ** exponent).toFixed(exponent) }
    parts.push(t('revenue.perRecord', { amount: format.money(perRecord) }))
  }
  return parts.join(' · ')
})

/** Index of the chart's measure in the state; the first when `m` names none of them. */
const selected = computed(() => {
  const index = (state.value?.measures ?? []).findIndex((measure) => measure === route.query.m)
  return Math.max(0, index)
})
const select = (index: number) => {
  const measure = state.value?.measures[index]
  if (typeof measure !== 'string' || index === selected.value) return
  void router.push({ query: { ...route.query, m: measure } })
}

/** The chart's measure in the chosen currency, its previous period narrowed the same way. */
const series = computed<Series | null>(() => {
  const source = result.value?.main.series
  const code = currency.value
  const at = selected.value
  if (!source || !code) return null
  const pick = (metric: Metric | undefined): Metric[] => (metric ? [only(metric, code)] : [])
  return {
    ...source,
    points: source.points.map((point) => {
      const comparison = point.compare?.[at]
      return {
        ...point,
        values: pick(point.values[at]),
        compare: comparison && [{ ...comparison, previous: only(comparison.previous, code) }],
      }
    }),
    projection: source.projection?.map((point) => ({ ...point, values: [point.values[at] ?? null] })),
  }
})
const chartMeasure = computed<Measure>(() => state.value?.measures[selected.value] ?? 'revenue_net')
/** Records have no currency, so with several currencies their series counts all of them. */
const chartLabel = computed(() => {
  const measure = format.measureLabel(chartMeasure.value)
  return chartMeasure.value === 'revenue_count' && currencies.value.length > 1
    ? t('revenue.chartAll', { measure })
    : t('revenue.chartIn', { measure, currency: currency.value ?? '' })
})
const clock = computed(() => (result.value ? chartClock(now.value.getTime(), result.value) : undefined))
const baseline = computed(() =>
  state.value && resolved.value ? baselineOf(state.value.compare, resolved.value) : null
)

/**
 * The breakdown in the chosen currency, with that currency's totals and without its column. Only
 * when every row is on this page; across pages it lists all currencies with their column.
 */
const table = computed(() => {
  const main = result.value?.main
  const code = currency.value
  const totals = strip.value
  if (!main) return null
  if (!breakdown.value || !code || !totals || main.row_count > main.rows.length) return { cut: main, implied: [] }
  const rows = main.rows.filter((row) => codeOf(row) === code)
  return {
    cut: { ...main, rows, row_count: rows.length, totals: totals.totals, compare: totals.compare },
    implied: ['currency' as const],
  }
})

/** The tracker call that records a purchase, for the empty state to copy (0002 Currencies and revenue). */
const trackCall = "umami.track('purchase', { revenue: 49.9, currency: 'EUR' })"
const copyTrackCall = async () => {
  await navigator.clipboard.writeText(trackCall)
  toast.success(t('secrets.copied'))
}

const records = computed(() => {
  const metric = result.value?.main.totals[indexOf('revenue_count')]
  return metric?.t === 'count' ? metric.v : 0
})
/** How the amounts are counted, behind the info icon beside the breakdown. */
const note = computed(() => t('revenue.note', { records: format.metric({ t: 'count', v: records.value }) }))

const breakdowns: DimensionField[] = [
  'channel',
  'referrer_domain',
  'utm_campaign',
  'country',
  'device',
  'browser',
  'path',
  'event_name',
]
const breakdown = computed(() => state.value?.dimensions[1] ?? null)
const isBreakdown = (value: unknown): value is DimensionField => breakdowns.some((b) => b === value)
const breakdownText = computed(() =>
  breakdown.value ? t('revenue.breakdownBy', { field: format.fieldLabel(breakdown.value) }) : t('revenue.byCurrency')
)
const onBreakdown = (value: unknown) => {
  const current = state.value
  if (!current) return
  const second = isBreakdown(value) ? [value] : []
  void screen.commit({
    ...current,
    dimensions: ['currency', ...second],
    sort: second.length ? [{ by: 'revenue_net', dir: 'desc' }] : [{ by: 'currency', dir: 'asc' }],
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
    :title="t('revenue.title')"
    :heading="{ title: t('revenue.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template #context>
      <ChoiceGroup
        v-model="view"
        :label="t('revenue.view.label')"
        :options="views"
      />
      <ChoiceGroup
        v-if="currency && currencies.length > 1"
        :model-value="currency"
        :label="t('revenue.currency')"
        :options="currencies.map((code) => ({ value: code, label: code }))"
        @update:model-value="(code) => (chosen = code)"
      />
    </template>
  </ExplorationHeader>

  <template v-if="state && !screen.refusal.value">
    <EmptyState
      v-if="result && currencies.length === 0"
      icon="lucide:receipt"
      :title="t('revenue.empty')"
      :description="t('revenue.emptyHint')"
    >
      <div class="flex w-full max-w-2xl items-start gap-2">
        <code
          class="min-w-0 flex-1 rounded-control bg-fill px-3 py-2 text-left font-mono text-sm break-words text-primary"
        >
          {{ trackCall }}
        </code>
        <Button
          variant="fill"
          class="shrink-0"
          @click="copyTrackCall"
        >
          <Icon
            name="lucide:copy"
            aria-hidden="true"
          />
          {{ t('secrets.copy') }}
        </Button>
      </div>
    </EmptyState>
    <RevenueSummary
      v-else-if="view === 'summary' && result && currency"
      :state="state"
      :result="result"
      :currency="currency"
      :net="amount('revenue_net')?.money ?? null"
      :gross="amount('revenue_gross')?.money ?? null"
      :refunds="amount('revenue_refunds')?.money ?? null"
      :records="stripCount"
      :compare="strip?.compare?.[indexOf('revenue_net')]"
      :annotations="annotations.list.data.value?.annotations ?? []"
    />
    <template v-else>
      <MeasureCards
        :measures="state.measures"
        :cut="strip"
        :visitor-label="null"
        :selected="selected"
        @select="select"
      />
      <p class="mt-2.5 min-h-5 text-sm text-muted tabular-nums">{{ facts }}</p>

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
          :annotations="annotations.list.data.value?.annotations ?? []"
          :intervals="granularities"
          :clock="clock"
          :baseline="baseline"
          @interval="screen.commit(setGranularity(state, $event))"
        />
        <Skeleton
          v-else
          class="h-80 w-full"
        />
      </Card>

      <div class="mb-3 flex flex-wrap items-center gap-2">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button
              variant="outline"
              size="sm"
              :aria-label="t('revenue.breakdownLabel', { value: breakdownText })"
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
              <DropdownMenuRadioItem value="none">{{ t('revenue.byCurrency') }}</DropdownMenuRadioItem>
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
          :tooltip="note"
          :aria-label="note"
          class="inline-flex size-7 cursor-help items-center justify-center rounded-control text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          <Icon
            name="lucide:info"
            aria-hidden="true"
          />
        </SimpleTooltip>
      </div>
      <ResultTable
        v-if="result && table"
        :state="state"
        :cut="table.cut"
        :implied="table.implied"
        :visitor-label="result.visitor_label"
        :split-fields="[]"
        @commit="screen.commit"
      />
      <Skeleton
        v-else
        class="h-64 w-full"
      />
    </template>
  </template>
</template>
