<script setup lang="ts">
import { useIntervalFn, useNow } from '@vueuse/core'
import { Translation } from 'vue-i18n'

import Icon from '~/components/Icon.vue'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplore } from '~/composables/useExplore'
import { useFormat } from '~/composables/useFormat'
import { useGoals } from '~/composables/useGoals'
import type { ExplorationState, Site } from '~/generated'
import { REPORT_GOALS_MAX } from '~/generated/limits'
import { goalPaceRow, goalPaceState, type GoalPaceRow } from '~/pages/overview/goal-pace'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { baselineOf, encodeState } from '~/services/exploration'

/**
 * The overview's goals panel: when the site has active goals, one `goals` report over the
 * overview's range and filter against a typical weekday, its own request beside the overview's
 * snapshot so neither waits for the other. Each row states the pace in words, draws completions
 * against the typical value by now, and opens the Goals report with the same scope. A projected
 * count is marked as an estimate and names its band and basis.
 */
const props = defineProps<{
  site: Site
  /** The overview's state; the panel follows its range and filter. */
  state: ExplorationState
}>()

const { t } = useI18n()
const numbers = useFormat()
const { message } = useErrorMessage()
const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })
const id = useId()

const goals = useGoals(() => props.site.id)
const saved = computed(() => goals.goals.value.slice(0, REPORT_GOALS_MAX))
const report = computed(() => (saved.value.length > 0 ? goalPaceState(props.state, saved.value) : null))
const { query } = useExplore(report)

const resolved = computed(() => query.data.value?.resolved ?? null)
const results = computed(() => {
  const output = query.data.value?.report
  return output?.kind === 'goals' ? output.goals : null
})
const rows = computed(() => {
  const range = resolved.value
  if (!results.value || !range) return null
  return saved.value.flatMap((goal, index) => {
    const result = results.value?.[index]
    return result ? [{ goal, ...goalPaceRow(result, range, now.value.getTime()) }] : []
  })
})
const live = computed(() => rows.value?.some((row) => row.live) ?? true)

const basis = computed<MessageKey>(() => {
  const range = resolved.value
  const baseline = range ? baselineOf('typical_weekday', range) : null
  return baseline?.kind === 'weekday' ? `overview.headline.basis.weekday.${baseline.weekday}` : 'overview.headline.basis.days'
})

const detail = (row: GoalPaceRow): { key: MessageKey; named: Record<string, string | number> } => {
  if (row.typical === null) return { key: 'overview.goals.detail.plain', named: { count: row.completions } }
  const named = { completions: numbers.integer(row.completions), typical: numbers.integer(row.typical) }
  return { key: row.live ? 'overview.goals.detail.soFar' : 'overview.goals.detail.ofTypical', named }
}

const stateKey = (row: GoalPaceRow): MessageKey => {
  switch (row.state.kind) {
    case 'behind':
      return row.live ? 'overview.goals.state.behind' : 'overview.goals.state.below'
    case 'ahead':
      return row.live ? (row.estimate ? 'overview.goals.state.aheadFor' : 'overview.goals.state.ahead') : 'overview.goals.state.above'
    case 'even':
      return row.live ? 'overview.goals.state.even' : 'overview.goals.state.same'
    case 'no_history':
      return 'overview.goals.state.noHistory'
    case 'none':
      return 'overview.goals.state.none'
  }
}

const tone = (row: GoalPaceRow) =>
  row.state.kind === 'behind' ? 'destructive' : row.state.kind === 'ahead' ? 'success' : 'muted'
const fillTone = { destructive: 'bg-destructive', success: 'bg-success', muted: 'bg-accent' } as const
const textTone = { destructive: 'text-destructive', success: 'text-success', muted: 'text-muted' } as const

const estimateText = (row: GoalPaceRow) => {
  const estimate = row.estimate
  if (!estimate) return null
  const band = t('overview.headline.band', { low: numbers.integer(estimate.low), high: numbers.integer(estimate.high) })
  const value = numbers.integer(estimate.value)
  const tooltip = `${band} ${t(basis.value, { count: estimate.basis_days })}`
  return { value, tooltip, label: `${t('overview.headline.estimate', { value })}. ${tooltip}` }
}

const link = computed(() =>
  report.value ? { name: 'goals', params: { siteId: props.site.id }, query: { s: encodeState(report.value) } } : null
)
const cellX = 'px-4 first:pl-4.5 last:pr-4.5'
</script>

<template>
  <Panel
    v-if="saved.length > 0"
    :id="id"
    :title="t('overview.goals.title')"
    :meta="t('overview.goals.count', { count: saved.length })"
    flush
  >
    <template #actions>
      <span class="truncate text-[0.875rem] text-muted max-sm:hidden lg:max-xl:hidden">
        {{ live ? t('overview.goals.tick') : t('overview.goals.tickPast') }}
      </span>
    </template>
    <table
      class="w-full table-fixed text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider"
      :aria-labelledby="id"
    >
      <colgroup>
        <col class="w-[46%] xl:w-60" />
        <col />
        <col class="w-[30%] xl:w-40" />
      </colgroup>
      <thead>
        <tr class="h-table-head bg-fill text-left text-[0.875rem] font-medium text-muted">
          <th
            scope="col"
            class="font-medium"
            :class="cellX"
          >
            <span class="flex items-center gap-2">
              <Icon
                name="lucide:flag"
                size="14"
                aria-hidden="true"
              />
              {{ t('overview.goals.columns.goal') }}
            </span>
          </th>
          <th
            scope="col"
            class="font-medium"
            :class="cellX"
          >
            {{ t('overview.goals.columns.pace') }}
          </th>
          <th
            scope="col"
            class="text-right font-medium"
            :class="cellX"
          >
            {{ t('overview.goals.columns.state') }}
          </th>
        </tr>
      </thead>
      <tbody v-if="rows">
        <tr
          v-for="row in rows"
          :key="row.goal.id"
          class="h-15.5 border-t border-divider"
        >
          <th
            scope="row"
            class="text-left font-normal"
            :class="cellX"
          >
            <RouterLink
              v-if="link"
              :to="link"
              class="block min-w-0 rounded-sm leading-tight focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
              :aria-label="t('overview.goals.open', { goal: row.goal.name })"
            >
              <span class="block truncate font-medium text-primary hover:underline">{{ row.goal.name }}</span>
              <span class="mt-1 block truncate text-[0.875rem] text-muted tabular-nums">
                {{ t(detail(row).key, detail(row).named) }}
              </span>
            </RouterLink>
          </th>
          <td :class="cellX">
            <span
              class="relative block h-1.5 rounded-[3px] bg-fill-strong"
              aria-hidden="true"
            >
              <span
                class="absolute inset-y-0 left-0 rounded-[3px] transition-[width] duration-300 ease-butter"
                :class="fillTone[tone(row)]"
                :style="{ width: `${row.fill * 100}%` }"
              />
              <span
                v-if="row.tick !== null"
                class="absolute -inset-y-1 border-l-[1.5px] border-dashed border-muted"
                :style="{ left: `${row.tick * 100}%` }"
              />
            </span>
          </td>
          <td
            class="text-right text-sm leading-snug font-medium"
            :class="[cellX, textTone[tone(row)]]"
          >
            <Translation
              v-if="stateKey(row) === 'overview.goals.state.aheadFor' && estimateText(row)"
              keypath="overview.goals.state.aheadFor"
              scope="global"
              tag="span"
            >
              <template #projection>
                <SimpleTooltip
                  :tooltip="estimateText(row)?.tooltip ?? ''"
                  as-child
                >
                  <button
                    type="button"
                    class="cursor-help rounded-sm font-semibold tabular-nums underline decoration-dashed underline-offset-3 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                    :aria-label="estimateText(row)?.label"
                  >
                    {{ estimateText(row)?.value }}
                  </button>
                </SimpleTooltip>
              </template>
            </Translation>
            <template v-else>
              {{ t(stateKey(row), 'count' in row.state ? { count: numbers.integer(row.state.count) } : undefined) }}
            </template>
          </td>
        </tr>
      </tbody>
      <tbody v-else-if="query.isError.value">
        <tr class="border-t border-divider">
          <td
            colspan="3"
            class="px-4.5 py-6 text-center text-sm text-muted"
          >
            {{ message(query.error.value) }}
          </td>
        </tr>
      </tbody>
      <tbody v-else>
        <tr
          v-for="goal in saved"
          :key="goal.id"
          class="h-15.5 border-t border-divider"
        >
          <td :class="cellX"><Skeleton class="h-4 w-3/4" /></td>
          <td :class="cellX"><Skeleton class="h-1.5 w-full" /></td>
          <td :class="cellX"><Skeleton class="ml-auto h-4 w-20" /></td>
        </tr>
      </tbody>
    </table>
  </Panel>
</template>
