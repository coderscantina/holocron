<script setup lang="ts">
import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import ChangeBadge from '~/components/exploration/ChangeBadge.vue'
import FreshnessNote from '~/components/exploration/FreshnessNote.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import ReportSummary from '~/components/report/ReportSummary.vue'
import RetentionSummary from '~/components/retention/RetentionSummary.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplore } from '~/composables/useExplore'
import type { ShareScope } from '~/api/resources/sharing'
import type { Card as BoardCardData, Cell, ExplorationState, Range } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { countryCode } from '~/lib/client-looks'
import { pageOf } from '~/lib/state-pages'
import { encodeState } from '~/services/exploration'

/**
 * One board card: the pinned state, computed like any exploration. A board-wide range replaces
 * the card's own and the card says so; every other part of the state stays as pinned.
 */
const props = defineProps<{
  card: BoardCardData
  /** The card's pinned state; cards without one are not rendered through this component. */
  pinned: ExplorationState
  override: Range | null
  siteName: string | null
  canEdit: boolean
  first: boolean
  last: boolean
  /** Reads as a share viewer; the card then has no menu, since its links lead into the app. */
  share?: ShareScope
}>()

const emit = defineEmits<{
  move: [delta: number]
  remove: []
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const format = useExplorationFormat()

const state = computed(() => (props.override ? { ...props.pinned, range: props.override } : props.pinned))
const { query } = useExplore(state, [], props.share)
const result = computed(() => query.data.value ?? null)
const label = (index: number) => {
  const measure = state.value.measures[index]
  if (!measure) return ''
  return (measure === 'visitor_days' || measure === 'visitors') && result.value
    ? format.measureLabel(result.value.visitor_label)
    : format.measureLabel(measure)
}
const series = computed(
  () =>
    (state.value.visualization === 'line' || state.value.visualization === 'bar') &&
    pageOf(state.value) !== 'revenue' &&
    result.value?.main.series
)
const rows = computed(() => result.value?.main.rows.slice(0, 5) ?? [])
const rowLabel = (key: Cell[]) =>
  key
    .flatMap((cell, index) => {
      const field = state.value.dimensions[index]
      return field ? [format.cellLabel(field, cell)] : []
    })
    .join(' · ')
/** The flag of the row's first country cell, shown in front of its label. */
const rowFlag = (key: Cell[]) =>
  key.reduce<string | null>((found, cell, index) => {
    const field = state.value.dimensions[index]
    return found ?? (field ? countryCode(field, cell) : null)
  }, null)
const exploreLink = computed(() => ({
  name: pageOf(state.value),
  params: { siteId: props.pinned.site },
  query: { s: encodeState(state.value) },
}))
</script>

<template>
  <Panel
    :title="card.title"
    :meta="[siteName, format.rangeLabel(state.range)].filter(Boolean).join(' · ')"
    class="h-full"
  >
    <template #actions>
      <SimpleTooltip
        v-if="card.exploration"
        :tooltip="t('boards.pinnedFrom')"
        class="text-muted"
      >
        <Icon
          name="lucide:bookmark"
          aria-hidden="true"
        />
        <span class="sr-only">{{ t('boards.pinnedFrom') }}</span>
      </SimpleTooltip>
      <DropdownMenu v-if="!share">
        <DropdownMenuTrigger as-child>
          <Button
            variant="ghost"
            size="toolbar"
            :aria-label="t('boards.cardActions', { title: card.title })"
          >
            <Icon name="lucide:ellipsis" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem as-child>
            <RouterLink :to="exploreLink">
              <Icon
                name="lucide:search"
                aria-hidden="true"
              />
              {{ exploreLink.name === 'explore' ? t('boards.open') : t('boards.openReport') }}
            </RouterLink>
          </DropdownMenuItem>
          <template v-if="canEdit">
            <DropdownMenuSeparator />
            <DropdownMenuItem
              :disabled="first"
              @select="emit('move', -1)"
            >
              <Icon
                name="lucide:arrow-up"
                aria-hidden="true"
              />
              {{ t('boards.moveUp', { title: card.title }) }}
            </DropdownMenuItem>
            <DropdownMenuItem
              :disabled="last"
              @select="emit('move', 1)"
            >
              <Icon
                name="lucide:arrow-down"
                aria-hidden="true"
              />
              {{ t('boards.moveDown', { title: card.title }) }}
            </DropdownMenuItem>
            <DropdownMenuItem @select="emit('remove')">
              <Icon
                name="lucide:trash-2"
                aria-hidden="true"
              />
              {{ t('boards.remove', { title: card.title }) }}
            </DropdownMenuItem>
          </template>
        </DropdownMenuContent>
      </DropdownMenu>
    </template>
    <div class="flex flex-col gap-3">
      <p
        v-if="query.isError.value"
        class="text-sm text-destructive"
        role="alert"
      >
        {{ message(query.error.value) }}
      </p>
      <template v-else-if="result">
        <dl class="grid grid-cols-2 gap-x-4 gap-y-2">
          <div
            v-for="(value, index) in result.main.totals"
            :key="index"
            class="min-w-0"
          >
            <dt class="truncate text-xs text-muted">{{ label(index) }}</dt>
            <dd class="flex flex-wrap items-baseline gap-x-2">
              <span class="text-lg font-semibold text-primary tabular-nums">{{ format.metric(value, true) }}</span>
              <span
                v-if="result.main.compare?.[index]"
                class="text-xs text-muted"
              >
                <ChangeBadge
                  :comparison="result.main.compare[index]"
                  :measure="state.measures[index]"
                />
              </span>
            </dd>
          </div>
        </dl>
        <RetentionSummary
          v-if="result.report?.kind === 'retention'"
          :result="result.report"
          :granularity="state.granularity"
        />
        <ReportSummary
          v-else-if="result.report"
          :report="result.report"
        />
        <TrendChart
          v-if="series && result.main.series"
          :series="result.main.series"
          :measures="state.measures.slice(0, 2)"
          :labels="[label(0), label(1)].filter(Boolean)"
          :visualization="state.visualization === 'line' ? 'line' : 'bar'"
          :annotations="[]"
        />
        <table
          v-if="rows.length > 0"
          class="w-full table-fixed text-sm"
        >
          <caption class="sr-only">{{ card.title }}</caption>
          <tbody>
            <tr
              v-for="(row, index) in rows"
              :key="index"
              class="border-t border-divider"
            >
              <th
                scope="row"
                class="py-1.5 text-left font-normal text-foreground"
              >
                <span class="flex min-w-0 items-center gap-2">
                  <CountryFlag
                    v-if="rowFlag(row.key)"
                    :code="rowFlag(row.key) ?? ''"
                  />
                  <span class="truncate">{{ rowLabel(row.key) }}</span>
                </span>
              </th>
              <td class="w-24 py-1.5 text-right font-medium text-primary tabular-nums">{{ format.metric(row.values[0]) }}</td>
            </tr>
          </tbody>
        </table>
        <FreshnessNote :result="result" />
      </template>
      <Skeleton
        v-else
        class="h-32 w-full"
      />
    </div>
  </Panel>
</template>
