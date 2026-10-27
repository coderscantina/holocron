<script setup lang="ts">
import ChangeBadge from '~/components/exploration/ChangeBadge.vue'
import { baselineKey } from '~/components/exploration/trend'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplore } from '~/composables/useExplore'
import type { Annotation, Comparison, DimensionField, ExplorationState, ExploreResult, Metric, Money } from '~/generated'
import { PAGE_SIZE_MAX } from '~/generated/limits'
import { weekdayLabels } from '~/lib/exploration-labels'
import { bestPayer, busiest, hourly, pace, sourceOf, traffic, trafficSources, type Source, type TrafficSource } from '~/lib/revenue-summary'
import { useI18n } from '~/plugins/i18n'
import { addCondition, baselineOf, containsNow, encodeState, include } from '~/services/exploration'

import PaceChart from './PaceChart.vue'
import SourceTable from './SourceTable.vue'

/**
 * Revenue in one currency at a glance. The column: net revenue with its change, gross, refunds
 * and net per record, the 24 local hours with the busiest three marked, and one sentence on when
 * money arrives and which source pays best. The canvas: the running total against the
 * comparison with the pace estimate to the end of the range, and the sources ranked by revenue
 * beside their share of visitors. The source is the `by` parameter, beside the state.
 */
const props = defineProps<{
  state: ExplorationState
  result: ExploreResult
  currency: string
  net: Money | null
  gross: Money | null
  refunds: Money | null
  records: number
  compare?: Comparison
  annotations: Annotation[]
}>()

const { t, locale } = useI18n()
const route = useRoute()
const router = useRouter()
const format = useExplorationFormat()
/** The server's now, as far as counted data reaches; the browser's clock can differ from it. */
const now = computed(() => props.result.freshness.watermark_at)

const exponent = computed(() => props.net?.exponent ?? 2)
const money = computed(() => {
  const options = { style: 'currency', currency: props.currency } as const
  return {
    exact: new Intl.NumberFormat(locale.value, { ...options, minimumFractionDigits: exponent.value, maximumFractionDigits: exponent.value }),
    compact: new Intl.NumberFormat(locale.value, { ...options, notation: 'compact', maximumFractionDigits: 1 }),
  }
})
/** Minor units of the chosen currency as an amount; estimates and rates are not whole units. */
const amount = (units: number, compact = false) => (compact ? money.value.compact : money.value.exact).format(units / 10 ** exponent.value)
const units = (value: Money | null) => (value ? Number(value.minor_units) : 0)

const sourceKey = computed(() => sourceOf(route.query.by))
const source = computed(() => trafficSources[sourceKey.value])
const sourceOptions = (Object.keys(trafficSources) as TrafficSource[]).map((key) => ({
  value: key,
  label: format.fieldLabel(trafficSources[key]),
}))
const chooseSource = (key: TrafficSource) => void router.push({ query: { ...route.query, by: key === 'channel' ? undefined : key } })

/** The page's state as one more read: every row on one page, without comparison. */
const read = (patch: Pick<ExplorationState, 'measures' | 'dimensions' | 'sort'>): ExplorationState => ({
  ...props.state,
  ...patch,
  compare: 'none',
  visualization: 'table',
  page: { size: PAGE_SIZE_MAX, index: 0 },
})
const byHour = useExplore(() =>
  read({ measures: ['revenue_net', 'revenue_count'], dimensions: ['currency', 'hour'], sort: [{ by: 'revenue_net', dir: 'desc' }] })
).query
/** Net, records and visitors with a revenue record, by currency and source. */
const paying = useExplore(() =>
  addCondition(
    read({
      measures: ['revenue_net', 'revenue_count', 'visitor_days'],
      dimensions: ['currency', source.value],
      sort: [{ by: 'revenue_net', dir: 'desc' }],
    }),
    { field: 'currency', cmp: 'is_set' }
  )
).query
/** All visitors by source, the denominator of shares, rates and conversion. */
const visitors = useExplore(() =>
  read({ measures: ['visitor_days'], dimensions: [source.value], sort: [{ by: 'visitor_days', dir: 'desc' }] })
).query
/**
 * The pace estimate needs the typical-weekday comparison (0015 Pace projection), a second read
 * of the same range, only while it contains now. A refusal, such as a range over 31 days,
 * leaves the chart without an estimate.
 */
const projected = useExplore((): ExplorationState | null =>
  containsNow(props.result.resolved, now.value)
    ? { ...props.state, measures: ['revenue_net', 'revenue_count'], dimensions: [], sort: [], compare: 'typical_weekday', visualization: 'line' }
    : null
).query

const hours = computed(() => hourly(byHour.data.value?.main.rows ?? [], props.currency))
const maxHour = computed(() => Math.max(1, ...hours.value))
const peak = computed(() => busiest(hours.value))
const inPeak = (hour: number) => {
  const stretch = peak.value
  return stretch ? (hour - stretch.start + 24) % 24 < stretch.hours : false
}
const clock = computed(() => new Intl.DateTimeFormat(locale.value, { hour: 'numeric', minute: '2-digit', timeZone: 'UTC' }))
const hourText = (hour: number) => clock.value.format(Date.UTC(2000, 0, 1, hour % 24))

const totalVisitors = computed(() => {
  const metric = visitors.data.value?.main.totals[0]
  return metric?.t === 'count' ? metric.v : 0
})
const ranked = computed<Source[] | null>(() => {
  const payingRows = paying.data.value?.main.rows
  const allRows = visitors.data.value?.main.rows
  if (!payingRows || !allRows) return null
  return traffic(payingRows, allRows, props.currency, { net: units(props.net), visitors: totalVisitors.value })
})
const siteRate = computed(() => (totalVisitors.value > 0 ? units(props.net) / totalVisitors.value : 0))
const best = computed(() => (ranked.value ? bestPayer(ranked.value, siteRate.value) : null))

/** One plain sentence: when the money arrives, and the source that pays the most per visitor. */
const takeaway = computed(() => {
  const stretch = peak.value
  const parts: string[] = []
  if (stretch) parts.push(t('revenue.summary.when', { from: hourText(stretch.start), until: hourText(stretch.start + stretch.hours) }))
  const payer = best.value
  if (payer?.key && payer.perVisitor !== null)
    parts.push(
      t(props.result.visitor_label === 'visitors' ? 'revenue.summary.bestPayer' : 'revenue.summary.bestPayerDay', {
        source: format.cellLabel(source.value, payer.key),
        amount: amount(payer.perVisitor),
        times: new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1 }).format(payer.perVisitor / siteRate.value),
        site: amount(siteRate.value),
      })
    )
  return parts.join(' ')
})

const refundShare = computed<Metric>(() => {
  const gross = units(props.gross)
  const refunded = Math.abs(units(props.refunds))
  return gross > 0 ? { t: 'ratio', v: refunded / gross, numerator: refunded, denominator: gross } : { t: 'unavailable', reason: 'no_denominator' }
})
const perRecord = computed(() => (props.records > 0 ? units(props.net) / props.records : null))
const composition = computed(() => {
  const kept = Math.max(0, units(props.net))
  const refunded = Math.abs(units(props.refunds))
  const whole = Math.max(1, kept + refunded)
  return { kept: (kept / whole) * 100, refunded: (refunded / whole) * 100 }
})

const baseline = computed(() => baselineOf(props.state.compare, props.result.resolved))
const baselineLabel = computed(() => {
  const value = baseline.value
  if (!value) return null
  return t(baselineKey(value), { weekday: value.kind === 'weekday' ? t(weekdayLabels[value.weekday]) : '' })
})
const comparedWith = computed(() => {
  const value = baseline.value
  if (!value) return null
  if (value.kind === 'weekday') return t('explore.scope.compare.weekday', { weekday: t(weekdayLabels[value.weekday]) })
  return value.kind === 'weekdays' ? t('explore.scope.compare.weekdays') : t('explore.scope.compare.previous')
})

const netIndex = computed(() => props.state.measures.indexOf('revenue_net'))
const paced = computed(() => {
  const series = props.result.main.series
  if (!series || netIndex.value < 0) return null
  const cut = projected.data.value?.main
  return pace(series, netIndex.value, props.currency, now.value, cut && { series: cut.series, total: cut.projection?.[0] })
})
const paceLead = computed(() => {
  const value = paced.value
  if (!value || value.previous === null) return null
  const gap = value.actual - value.previous
  return gap >= 0 ? t('revenue.summary.pace.ahead', { amount: amount(gap) }) : t('revenue.summary.pace.behind', { amount: amount(-gap) })
})
const paceEstimate = computed(() => {
  const estimate = paced.value?.estimate
  if (!estimate) return null
  return [
    t('revenue.summary.pace.estimate', { value: amount(estimate.value), low: amount(estimate.low), high: amount(estimate.high) }),
    t('explore.chart.basis.days', { count: estimate.basisDays }),
  ].join(' ')
})

/** Explore with this page's state narrowed to one source, revenue and visitors by currency. */
const exploreSource = (row: Source) => {
  if (!row.key) return null
  const narrowed = include(
    {
      ...props.state,
      measures: ['revenue_net', 'revenue_count', 'visitor_days'],
      dimensions: ['currency'],
      sort: [{ by: 'revenue_net', dir: 'desc' }],
    },
    source.value,
    row.key
  )
  return { name: 'explore', params: { siteId: props.state.site }, query: { s: encodeState(narrowed) } }
}
const exploreHour = (hour: number) => {
  const narrowed = addCondition(
    { ...props.state, dimensions: ['currency', source.value], sort: [{ by: 'revenue_net', dir: 'desc' }], visualization: 'table' },
    { field: 'hour', cmp: 'eq', value: { t: 'integer', v: hour } }
  )
  return { name: 'explore', params: { siteId: props.state.site }, query: { s: encodeState(narrowed) } }
}
</script>

<template>
  <div class="grid gap-6 lg:grid-cols-[minmax(16rem,22rem)_1fr] lg:gap-0">
    <section
      class="flex min-w-0 flex-col lg:border-r lg:border-hairline lg:pr-8"
      :aria-label="t('revenue.title')"
    >
      <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ net ? format.money(net) : '–' }}</p>
      <p
        v-if="compare && comparedWith"
        class="mt-3 flex flex-wrap items-baseline gap-x-1.5 text-[0.9375rem] text-muted"
      >
        <ChangeBadge
          :comparison="compare"
          measure="revenue_net"
        />
        {{ comparedWith }}
      </p>

      <div class="mt-6 border-t border-hairline pt-5">
        <p class="mb-2.5 text-[0.8125rem] text-muted">{{ t('revenue.summary.composition') }}</p>
        <div
          class="flex h-2.5 gap-0.5 overflow-hidden rounded-xs"
          aria-hidden="true"
        >
          <span
            class="bg-series-secondary"
            :style="{ width: `${composition.kept}%` }"
          />
          <span
            class="bg-[repeating-linear-gradient(135deg,var(--destructive)_0_2px,transparent_2px_5px)]"
            :style="{ width: `${composition.refunded}%` }"
          />
        </div>
        <dl class="mt-3 grid grid-cols-[1fr_auto] gap-y-1.5 text-sm tabular-nums">
          <dt class="text-muted">{{ format.measureLabel('revenue_gross') }}</dt>
          <dd class="m-0 text-right text-primary">{{ gross ? format.money(gross) : '–' }}</dd>
          <dt class="text-muted">{{ t('revenue.summary.refunds', { share: format.metric(refundShare) }) }}</dt>
          <dd class="m-0 text-right text-primary">{{ refunds ? format.money(refunds) : '–' }}</dd>
          <dt class="text-muted">{{ t('revenue.summary.perRecord', { count: format.metric({ t: 'count', v: records }) }) }}</dt>
          <dd class="m-0 text-right text-primary">{{ perRecord === null ? '–' : amount(perRecord) }}</dd>
        </dl>
      </div>

      <figure class="m-0 mt-6 border-t border-hairline pt-5">
        <figcaption class="mb-2.5 text-[0.8125rem] text-muted">{{ t('revenue.summary.byHour') }}</figcaption>
        <div class="flex h-16 items-end gap-[3px]">
          <RouterLink
            v-for="(hourUnits, hour) in hours"
            :key="hour"
            :to="exploreHour(hour)"
            class="group flex h-full flex-1 items-end rounded-t-[1px] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            :aria-label="t('revenue.summary.hour', { from: hourText(hour), until: hourText(hour + 1), amount: amount(hourUnits) })"
            :title="`${hourText(hour)} · ${amount(hourUnits)}`"
          >
            <span
              class="block min-h-px w-full rounded-t-[1px] group-hover:opacity-80"
              :class="inPeak(hour) ? 'bg-accent' : hourUnits > 0 ? 'bg-series-comparison' : 'bg-hairline'"
              :style="{ height: `${(hourUnits / maxHour) * 100}%` }"
            />
          </RouterLink>
        </div>
        <p
          class="mt-2 grid grid-cols-4 text-[0.8125rem] text-muted tabular-nums"
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
    </section>

    <div class="flex min-w-0 flex-col gap-6 lg:pl-6">
      <Panel
        v-if="paced"
        :title="t('revenue.summary.pace.title')"
        :meta="format.rangeLabel(state.range)"
      >
        <p
          v-if="paceLead || paceEstimate"
          class="mb-5 text-[0.9375rem] text-primary"
        >
          {{ paceLead }}
          <span
            v-if="paceEstimate"
            class="text-sm text-muted"
          >
            {{ paceEstimate }}
          </span>
        </p>
        <PaceChart
          :pace="paced"
          :granularity="result.main.series?.granularity ?? state.granularity"
          :end="result.resolved.end"
          :annotations="annotations"
          :amount="amount"
          :label="t('revenue.summary.pace.label', { currency })"
          :previous-label="baselineLabel"
        />
      </Panel>

      <Panel :title="t('revenue.summary.traffic.title')">
        <div class="-mx-4.5 mb-4 overflow-x-auto px-4.5">
          <ChoiceGroup
            :model-value="sourceKey"
            :label="t('revenue.summary.traffic.by')"
            :options="sourceOptions"
            @update:model-value="chooseSource"
          />
        </div>
        <template v-if="ranked">
          <SourceTable
            v-if="ranked.length > 0"
            :sources="ranked"
            :field="source"
            :visitor-label="result.visitor_label"
            :amount="amount"
            :to="exploreSource"
          />
          <p
            v-else
            class="py-6 text-center text-sm text-muted"
          >
            {{ t('revenue.summary.traffic.empty') }}
          </p>
          <p class="mt-3 text-[0.8125rem] text-muted">{{ t('revenue.summary.traffic.note') }}</p>
        </template>
        <Skeleton
          v-else
          class="h-72 w-full"
        />
      </Panel>
    </div>
  </div>
</template>
