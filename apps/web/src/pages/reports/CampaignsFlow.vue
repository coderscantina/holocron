<script setup lang="ts">
import FlowDiagram from '~/components/flow/FlowDiagram.vue'
import FlowFocus from '~/components/flow/FlowFocus.vue'
import type { FlowDiagramLink, FlowDiagramNode } from '~/components/flow/layout'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import StageView from '~/components/stage/StageView.vue'
import { EmptyState } from '~/components/ui/empty-state'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { Cut, DimensionField, ExplorationState, Money } from '~/generated'
import { fieldLabels } from '~/lib/exploration-labels'
import { amountIn, mainCurrency, pairFlow, sumIn, type PairNode } from '~/lib/pair-flow'
import { useI18n } from '~/plugins/i18n'
import { eventsFor, fieldEquals, sessionsFor } from '~/services/reports'

/**
 * Campaign traffic as a flow from channel to campaign value, sized by sessions or by net
 * revenue, so it shows where the money comes from. `cut` has the channel and `field` as its
 * dimensions and sessions and net revenue as its measures. Sessions without a value of
 * `field` are left out of the flow and counted under it.
 */
const props = defineProps<{ state: ExplorationState; cut: Cut; field: DimensionField }>()

const { t } = useI18n()
const format = useFormat()
const explorationFormat = useExplorationFormat()

const KEEP = 7

interface Row {
  channel: string | null
  value: string | null
  sessions: number
  revenue: Money[]
}
const rows = computed(() =>
  props.cut.rows.map((row): Row => {
    const [channel, value] = row.key
    const [sessions, revenue] = row.values
    return {
      channel: channel?.t === 'string' ? channel.v : null,
      value: value?.t === 'string' ? value.v : null,
      sessions: sessions?.t === 'count' ? sessions.v : 0,
      revenue: revenue?.t === 'money' ? revenue.v : [],
    }
  })
)
const tagged = computed(() => rows.value.filter((row) => row.value !== null))
const untagged = computed(() => rows.value.filter((row) => row.value === null).reduce((sum, row) => sum + row.sessions, 0))

const currency = computed(() => mainCurrency(tagged.value.flatMap((row) => row.revenue)))
const currencies = computed(() => new Set(tagged.value.flatMap((row) => row.revenue.map((money) => money.currency))).size)
type Weight = 'revenue' | 'sessions'
const chosen = ref<Weight | null>(null)
const weight = computed<Weight>(() => (currency.value ? (chosen.value ?? 'revenue') : 'sessions'))
const weights = computed(() => [
  { value: 'revenue' as const, label: t('reports.campaigns.flow.weight.revenue') },
  { value: 'sessions' as const, label: t('reports.campaigns.flow.weight.sessions') },
])
const valueOf = (row: Row) => (weight.value === 'revenue' ? amountIn(row.revenue, currency.value) : row.sessions)

const NO_CHANNEL = '\u0000'
const flow = computed(() =>
  pairFlow(
    tagged.value.map((row) => ({ from: row.channel ?? NO_CHANNEL, to: row.value ?? '', value: valueOf(row) })),
    { from: KEEP, to: KEEP }
  )
)
const rowsOf = (node: PairNode) =>
  tagged.value.filter((row) => {
    const key = node.side === 'from' ? (row.channel ?? NO_CHANNEL) : row.value
    return node.key === null ? !flow.value.nodes.some((other) => other.side === node.side && other.key === key) : key === node.key
  })
const moneyOf = (list: readonly Row[]) => (currency.value ? sumIn(list.flatMap((row) => row.revenue), currency.value) : null)
const figure = (list: readonly Row[]) => {
  if (weight.value === 'sessions') return format.compact(list.reduce((sum, row) => sum + row.sessions, 0))
  const money = moneyOf(list)
  return money ? explorationFormat.money(money) : '–'
}
const label = (node: PairNode) => {
  if (node.key === null) return node.side === 'from' ? t('reports.campaigns.flow.otherChannels') : t('reports.campaigns.flow.otherValues')
  if (node.side === 'to') return node.key
  return node.key === NO_CHANNEL ? t('explore.table.notSet') : explorationFormat.textLabel('channel', node.key)
}

const selected = ref<string | null>(null)
watch(
  () => [props.cut, weight.value] as const,
  () => (selected.value = null)
)
const top = computed(() => flow.value.nodes.find((node) => node.side === 'to' && node.key !== null) ?? null)
const nodes = computed(() =>
  flow.value.nodes.map(
    (node): FlowDiagramNode => ({
      id: node.id,
      column: node.column,
      label: label(node),
      figure: figure(rowsOf(node)),
      tone: node.key === null ? 'quiet' : node === top.value && !selected.value && weight.value === 'revenue' ? 'accent' : undefined,
    })
  )
)
const links = computed(() => {
  const focus = selected.value ?? top.value?.id
  return flow.value.links.map(
    (link): FlowDiagramLink => ({ ...link, emphasis: link.source === focus || link.target === focus ? link.value : 0 })
  )
})

const total = computed(() => flow.value.nodes.filter((node) => node.side === 'from').reduce((sum, node) => sum + node.value, 0))
const headline = computed(() => figure(tagged.value))
const channels = computed(() => flow.value.nodes.filter((node) => node.side === 'from'))
const share = (value: number) => (total.value > 0 ? value / total.value : 0)
const leader = computed(() => {
  const channel = channels.value.find((node) => node.key !== null)
  if (!channel) return null
  const into = flow.value.links
    .filter((link) => link.source === channel.id)
    .sort((a, b) => b.value - a.value)
    .map((link) => flow.value.nodes.find((node) => node.id === link.target))
    .find((node) => node?.key !== null && node !== undefined)
  return { channel: label(channel), share: format.percent(share(channel.value)), value: into ? label(into) : null }
})

const focused = computed(() => {
  const node = flow.value.nodes.find((candidate) => candidate.id === selected.value)
  if (!node) return null
  const list = rowsOf(node)
  const sessions = list.reduce((sum, row) => sum + row.sessions, 0)
  const money = moneyOf(list)
  const drillField = node.side === 'from' ? 'channel' : props.field
  const key = node.key === NO_CHANNEL ? null : node.key
  const condition = node.key === null ? null : [fieldEquals(props.state, drillField, key)]
  return {
    title: label(node),
    summary: [
      t('reports.campaigns.flow.sessions', { count: format.integer(sessions) }),
      ...(money ? [t('reports.campaigns.flow.revenue', { amount: explorationFormat.money(money) })] : []),
      t('reports.campaigns.flow.share', { share: format.percent(share(node.value)) }),
    ].join(' · '),
    sessions: condition ? sessionsFor(props.state, condition) : undefined,
    events: condition ? eventsFor(props.state, condition) : undefined,
  }
})
</script>

<template>
  <StageView :label="t('reports.campaigns.flow.rail')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ headline }}</p>
        <p class="mt-3 text-[0.9375rem] text-muted">
          {{ weight === 'revenue' ? t('reports.campaigns.flow.headline.revenue', { field: t(fieldLabels[field]) }) : t('reports.campaigns.flow.headline.sessions', { field: t(fieldLabels[field]) }) }}
        </p>
        <p
          v-if="leader"
          class="mt-5 text-[0.9375rem] leading-relaxed text-primary"
        >
          {{
            leader.value
              ? t('reports.campaigns.flow.leader', { channel: leader.channel, share: leader.share, value: leader.value })
              : t('reports.campaigns.flow.leaderOnly', { channel: leader.channel, share: leader.share })
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

      <h2 class="pt-5 pb-1 text-sm font-medium text-muted">{{ t('reports.campaigns.flow.channels') }}</h2>
      <ol class="min-h-0 flex-1 overflow-y-auto">
        <li
          v-for="(node, index) in channels"
          :key="node.id"
        >
          <button
            type="button"
            class="grid w-full grid-cols-[1fr_auto] items-center gap-x-3 gap-y-1 rounded-sm py-2 text-left text-[0.9375rem] hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring"
            :aria-pressed="selected === node.id"
            @click="selected = selected === node.id ? null : node.id"
          >
            <span
              class="min-w-0 truncate"
              :class="node.key === null ? 'text-muted' : 'text-primary'"
            >
              {{ label(node) }}
            </span>
            <span class="shrink-0 text-muted tabular-nums">{{ figure(rowsOf(node)) }}</span>
            <span
              class="col-span-2 h-1 overflow-hidden rounded-full bg-hairline"
              aria-hidden="true"
            >
              <span
                class="block h-full rounded-full"
                :class="selected === node.id || (!selected && index === 0 && weight === 'revenue') ? 'bg-accent' : 'bg-series-comparison'"
                :style="{ width: `${share(node.value) * 100}%` }"
              />
            </span>
          </button>
        </li>
      </ol>
      <p
        v-if="untagged > 0 || currencies > 1"
        class="pt-3 text-xs text-muted"
      >
        <template v-if="untagged > 0">
          {{ t('reports.campaigns.flow.untagged', { count: format.integer(untagged), field: t(fieldLabels[field]) }) }}
        </template>
        <template v-if="currencies > 1 && currency">
          {{ t('flow.currency', { currency }) }}
        </template>
      </p>
    </template>

    <div class="flex h-[32rem] flex-col lg:h-full">
      <div class="mb-4 shrink-0">
        <slot name="toolbar" />
      </div>
      <p class="mb-3 flex shrink-0 justify-between gap-4 pr-12 text-xs text-muted">
        <span>{{ t('reports.campaigns.flow.columns', { field: t(fieldLabels[field]) }) }}</span>
        <span>{{ t('reports.campaigns.flow.hint') }}</span>
      </p>
      <div class="min-h-0 flex-1 pb-12">
        <EmptyState
          v-if="tagged.length === 0"
          icon="lucide:megaphone"
          :title="t('reports.campaigns.empty')"
          :description="t('reports.campaigns.emptyHint')"
        />
        <FlowDiagram
          v-else
          :nodes="nodes"
          :links="links"
          :label="t('reports.campaigns.flow.label', { field: t(fieldLabels[field]) })"
          :selected="selected"
          :min-column-width="220"
          @select="(id) => (selected = id)"
        />
      </div>
    </div>
    <template
      v-if="currency"
      #controls
    >
      <ChoiceGroup
        :model-value="weight"
        :label="t('reports.campaigns.flow.weight.label')"
        :options="weights"
        @update:model-value="(value) => (chosen = value)"
      />
    </template>
  </StageView>
</template>
