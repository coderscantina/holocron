<script setup lang="ts">
import FlowDiagram from '~/components/flow/FlowDiagram.vue'
import FlowFocus from '~/components/flow/FlowFocus.vue'
import type { FlowDiagramLink, FlowDiagramNode } from '~/components/flow/layout'
import StageView from '~/components/stage/StageView.vue'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { AttributedConversion, AttributionModel, AttributionResult, Conversion, ExplorationState, Money } from '~/generated'
import { fieldLabels } from '~/lib/exploration-labels'
import { amountIn, mainCurrency, pairFlow, sumIn, type PairNode } from '~/lib/pair-flow'
import { useI18n } from '~/plugins/i18n'
import { attributionOf, eventsFor, fieldEquals, sessionsFor } from '~/services/reports'

/**
 * Attribution as a flow from the credited touch values to the conversions, sized by
 * conversions, with the bands into revenue in the accent. The column beside puts the
 * report's model next to the other one (`other`, the same report under the other model), so
 * a channel that opens journeys but rarely closes them stands out.
 */
const props = defineProps<{
  state: ExplorationState
  report: AttributionResult
  other: AttributionResult | null
  conversions: Conversion[]
  names: string[]
}>()

const { t } = useI18n()
const format = useFormat()
const explorationFormat = useExplorationFormat()

const KEEP = 7
const LIST = 6
const DIRECT = '\u0000direct'
const NOT_SET = '\u0000unset'

const attribution = computed(() => attributionOf(props.state))
const model = computed<AttributionModel>(() => attribution.value?.model ?? 'last_touch')
const otherModel = computed<AttributionModel>(() => (model.value === 'last_touch' ? 'first_touch' : 'last_touch'))
const fieldLabel = computed(() => t(fieldLabels[attribution.value?.touch_field ?? 'channel']))

interface Credit {
  key: string
  conversions: number
  revenue: Money[]
}
const credits = (result: AttributedConversion): Credit[] => [
  ...result.touches.map((row) => ({
    key: row.touch.t === 'string' ? row.touch.v : NOT_SET,
    conversions: row.attributed.conversions,
    revenue: row.attributed.revenue,
  })),
  { key: DIRECT, conversions: result.direct.conversions, revenue: result.direct.revenue },
]

/** The conversion the column is about: revenue when the report has it, else the first. */
const primary = computed(() => Math.max(0, props.conversions.findIndex((conversion) => conversion.kind === 'revenue')))
const isRevenue = computed(() => props.conversions[primary.value]?.kind === 'revenue')
const currency = computed(() => {
  const result = props.report.conversions[primary.value]
  return isRevenue.value && result ? mainCurrency(credits(result).flatMap((credit) => credit.revenue)) : null
})
const currencies = computed(() => {
  const result = props.report.conversions[primary.value]
  return isRevenue.value && result ? new Set(credits(result).flatMap((credit) => credit.revenue.map((money) => money.currency))).size : 0
})
const worth = (credit: Credit) => (currency.value ? amountIn(credit.revenue, currency.value) : credit.conversions)

const flow = computed(() =>
  pairFlow(
    props.report.conversions.flatMap((result, index) =>
      credits(result).map((credit) => ({ from: credit.key, to: String(index), value: credit.conversions }))
    ),
    { from: KEEP, to: KEEP }
  )
)
const touchLabel = (key: string | null) => {
  if (key === null) return t('reports.attribution.flow.otherTouches')
  if (key === DIRECT) return t('reports.attribution.direct')
  if (key === NOT_SET) return t('explore.table.notSet')
  return attribution.value?.touch_field === 'channel' ? explorationFormat.textLabel('channel', key) : key
}
const revenueOf = (index: number) => {
  const result = props.report.conversions[index]
  const code = result ? mainCurrency(credits(result).flatMap((credit) => credit.revenue)) : null
  return result && code ? sumIn(credits(result).flatMap((credit) => credit.revenue), code) : null
}
const nodeFigure = (node: PairNode) => {
  if (node.side === 'to' && node.key !== null && props.conversions[Number(node.key)]?.kind === 'revenue') {
    const money = revenueOf(Number(node.key))
    if (money) return explorationFormat.money(money)
  }
  return format.compact(node.value)
}

const selected = ref<string | null>(null)
watch(
  () => props.report,
  () => (selected.value = null)
)
const revenueNode = computed(() =>
  flow.value.nodes.find((node) => node.side === 'to' && node.key !== null && props.conversions[Number(node.key)]?.kind === 'revenue')
)
const nodes = computed(() =>
  flow.value.nodes.map(
    (node): FlowDiagramNode => ({
      id: node.id,
      column: node.column,
      label: node.side === 'from' ? touchLabel(node.key) : node.key === null ? '' : (props.names[Number(node.key)] ?? ''),
      figure: nodeFigure(node),
      tone: node === revenueNode.value ? 'accent' : node.key === null || node.key === DIRECT ? 'quiet' : undefined,
    })
  )
)
/** Without a selection: the value the takeaway names, else the largest outside touch. */
const lead = computed(() => {
  const touches = flow.value.nodes.filter((node) => node.side === 'from')
  return (touches.find((node) => node.key === shift.value?.key) ?? touches.find((node) => node.key !== null && node.key !== DIRECT))?.id
})
const links = computed(() => {
  const focus = selected.value ?? lead.value
  return flow.value.links.map(
    (link): FlowDiagramLink => ({ ...link, emphasis: link.source === focus || link.target === focus ? link.value : 0 })
  )
})

/** The primary conversion's credit per touch under both models, as shares of each total. */
const shares = (result: AttributionResult | null) => {
  const conversion = result?.conversions[primary.value]
  if (!conversion) return new Map<string, number>()
  const list = credits(conversion)
  const total = list.reduce((sum, credit) => sum + worth(credit), 0)
  return new Map(list.map((credit) => [credit.key, total > 0 ? worth(credit) / total : 0]))
}
const current = computed(() => shares(props.report))
const compared = computed(() => (props.other ? shares(props.other) : null))
const rows = computed(() =>
  [...current.value]
    .sort(([a, x], [b, y]) => y - x || (a < b ? -1 : 1))
    .slice(0, LIST)
    .map(([key, share]) => ({ key, label: touchLabel(key), share, other: compared.value?.get(key) ?? 0 }))
)
const shift = computed(() => {
  const all = compared.value
  if (!all) return null
  const keys = new Set([...current.value.keys(), ...all.keys()])
  const [key, gap] = [...keys]
    .filter((key) => key !== DIRECT)
    .map((key): [string, number] => [key, (all.get(key) ?? 0) - (current.value.get(key) ?? 0)])
    .reduce<[string, number]>((top, entry) => (Math.abs(entry[1]) > Math.abs(top[1]) ? entry : top), ['', 0])
  if (Math.abs(gap) < 0.01) return null
  return { key, label: touchLabel(key), now: format.percent(current.value.get(key) ?? 0), alt: format.percent(all.get(key) ?? 0) }
})

const headline = computed(() => {
  const result = props.report.conversions[primary.value]
  if (!result) return '–'
  if (currency.value) {
    const money = sumIn(credits(result).flatMap((credit) => credit.revenue), currency.value)
    if (money) return explorationFormat.money(money)
  }
  return format.integer(credits(result).reduce((sum, credit) => sum + credit.conversions, 0))
})

const focused = computed(() => {
  const node = flow.value.nodes.find((candidate) => candidate.id === selected.value)
  if (!node) return null
  const total = flow.value.nodes.filter((other) => other.side === node.side).reduce((sum, other) => sum + other.value, 0)
  const parts = [
    t('reports.attribution.flow.conversions', { count: format.integer(node.value) }),
    t('reports.attribution.flow.share', { share: format.percent(total > 0 ? node.value / total : 0) }),
  ]
  const field = attribution.value?.touch_field
  const touch = node.side === 'from' && node.key !== null && node.key !== DIRECT && field
  const condition = touch ? [fieldEquals(props.state, field, node.key === NOT_SET ? null : node.key)] : null
  return {
    title: nodes.value.find((candidate) => candidate.id === node.id)?.label ?? '',
    summary: parts.join(' · '),
    sessions: condition ? sessionsFor(props.state, condition) : undefined,
    events: condition ? eventsFor(props.state, condition) : undefined,
  }
})
const select = (key: string) => {
  const id = flow.value.nodes.find((node) => node.side === 'from' && node.key === key)?.id ?? null
  selected.value = selected.value === id ? null : id
}
</script>

<template>
  <StageView :label="t('reports.attribution.flow.rail')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ headline }}</p>
        <p class="mt-3 text-[0.9375rem] text-muted">
          {{
            isRevenue
              ? t('reports.attribution.flow.headline.revenue', { model: t(`reports.attribution.models.${model}`) })
              : t('reports.attribution.flow.headline.goal', { name: names[primary] ?? '', model: t(`reports.attribution.models.${model}`) })
          }}
        </p>
        <p
          v-if="shift"
          class="mt-5 text-[0.9375rem] leading-relaxed text-primary"
        >
          {{
            t(`reports.attribution.flow.shift.${model}`, {
              value: shift.label,
              now: shift.now,
              alt: shift.alt,
              what: isRevenue ? t('reports.attribution.flow.what.revenue') : t('reports.attribution.flow.what.conversions'),
            })
          }}
        </p>
      </div>

      <FlowFocus
        v-if="focused"
        :title="focused.title"
        :summary="focused.summary"
        :sessions="focused.sessions"
        :events="focused.events"
        @clear="selected = null"
      />

      <div class="flex items-baseline justify-between gap-3 pt-5 pb-2 text-sm">
        <h2 class="font-medium text-muted">{{ t('reports.attribution.flow.byTouch', { field: fieldLabel }) }}</h2>
        <p
          v-if="compared"
          class="flex shrink-0 items-center gap-3 text-xs text-muted"
        >
          <span class="flex items-center gap-1.5">
            <span
              class="h-1.5 w-3 rounded-full bg-primary"
              aria-hidden="true"
            />
            {{ t(`reports.attribution.models.${model}`) }}
          </span>
          <span class="flex items-center gap-1.5">
            <span
              class="h-1.5 w-3 rounded-full bg-series-comparison"
              aria-hidden="true"
            />
            {{ t(`reports.attribution.models.${otherModel}`) }}
          </span>
        </p>
      </div>
      <ol class="min-h-0 flex-1 overflow-y-auto">
        <li
          v-for="row in rows"
          :key="row.key"
        >
          <button
            type="button"
            class="grid w-full grid-cols-[1fr_auto] items-center gap-x-3 gap-y-1 rounded-sm py-2 text-left text-[0.9375rem] hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring"
            :aria-pressed="flow.nodes.some((node) => node.id === selected && node.key === row.key)"
            :aria-label="
              compared
                ? t('reports.attribution.flow.rowCompared', {
                    value: row.label,
                    now: format.percent(row.share),
                    model: t(`reports.attribution.models.${model}`),
                    alt: format.percent(row.other),
                    other: t(`reports.attribution.models.${otherModel}`),
                  })
                : `${row.label}, ${format.percent(row.share)}`
            "
            @click="select(row.key)"
          >
            <span
              class="min-w-0 truncate"
              :class="row.key === DIRECT ? 'text-muted' : 'text-primary'"
            >
              {{ row.label }}
            </span>
            <span class="shrink-0 text-muted tabular-nums">
              {{ format.percent(row.share) }}
              <template v-if="compared">
                <span
                  class="px-0.5"
                  aria-hidden="true"
                >
                  ·
                </span>
                <span :class="Math.abs(row.other - row.share) >= 0.05 ? 'font-semibold text-accent-text' : ''">{{ format.percent(row.other) }}</span>
              </template>
            </span>
            <span
              class="col-span-2 grid gap-0.5"
              aria-hidden="true"
            >
              <span class="h-1 overflow-hidden rounded-full bg-hairline">
                <span
                  class="block h-full rounded-full bg-primary"
                  :style="{ width: `${row.share * 100}%` }"
                />
              </span>
              <span
                v-if="compared"
                class="h-1 overflow-hidden rounded-full bg-hairline"
              >
                <span
                  class="block h-full rounded-full bg-series-comparison"
                  :style="{ width: `${row.other * 100}%` }"
                />
              </span>
            </span>
          </button>
        </li>
      </ol>
      <p
        v-if="currencies > 1 && currency"
        class="pt-3 text-xs text-muted"
      >
        {{ t('flow.currency', { currency }) }}
      </p>
    </template>

    <div class="flex h-[36rem] flex-col lg:h-full">
      <div class="mb-4 shrink-0">
        <slot name="toolbar" />
      </div>
      <p class="mb-3 flex shrink-0 justify-between gap-4 pr-12 text-xs text-muted">
        <span>{{ t('reports.attribution.flow.columns', { field: fieldLabel }) }}</span>
        <span>{{ t('reports.attribution.flow.hint') }}</span>
      </p>
      <div class="min-h-0 flex-1 pb-12">
        <FlowDiagram
          :nodes="nodes"
          :links="links"
          :label="t('reports.attribution.flow.label', { field: fieldLabel })"
          :selected="selected"
          :min-column-width="220"
          @select="(id) => (selected = id)"
        />
      </div>
    </div>
  </StageView>
</template>
