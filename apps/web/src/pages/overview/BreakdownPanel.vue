<script setup lang="ts">
import { ToggleGroupItem, ToggleGroupRoot, type AcceptableValue } from 'reka-ui'

import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { Cell, Cut, DimensionField, ExplorationState, Field } from '~/generated'
import { countryCode, valueIcon } from '~/lib/client-looks'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { canonical, exclude, include } from '~/services/exploration'

import { breakdownRows, sharePips, shownRows, type BreakdownMode } from './breakdown'

/**
 * One overview breakdown in the table pattern: the top values of one field by visitor-days,
 * or in Movers mode the same page sorted by absolute change against the comparison with a
 * diverging bar. Rows under the quiet threshold stay listed in faint text. Referrers carry a
 * 5-pip share meter. Every row
 * keeps, excludes or explores its value from a menu. `limit` rows show out of the cut's page.
 */
const props = withDefaults(
  defineProps<{
    title: string
    /** Plural message for the faint count, placeholder `count`. */
    count: MessageKey
    field: Field
    state: ExplorationState
    cut: Cut | null
    limit?: number
    /** Sibling breakdowns this panel switches between, shown in place of the title. */
    views?: { value: string; label: string }[]
    view?: string
    viewsLabel?: string
    /** Offers opening Explore split by the field; off where there is no Explore, as for links. */
    explorable?: boolean
  }>(),
  { limit: 10, explorable: true }
)

const emit = defineEmits<{
  commit: [state: ExplorationState]
  explore: [field: Field]
  view: [view: string]
}>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()
const id = useId()

const fieldIcons: Partial<Record<DimensionField, string>> = {
  path: 'lucide:file',
  referrer_domain: 'lucide:link',
  browser: 'lucide:globe',
  os: 'lucide:monitor',
  device: 'lucide:smartphone',
  country: 'lucide:map-pin',
}
const headIcon = computed(() => (typeof props.field === 'string' ? (fieldIcons[props.field] ?? null) : null))
const pips = computed(() => props.field === 'referrer_domain')

const picked = ref<BreakdownMode>('top')
const comparable = computed(() => props.state.compare !== 'none')
/** Movers fall back to Top while the state has no comparison. */
const mode = computed<BreakdownMode>(() => (comparable.value ? picked.value : 'top'))
const isMode = (value: AcceptableValue | AcceptableValue[]): value is BreakdownMode => value === 'top' || value === 'movers'
const pick = (value: AcceptableValue | AcceptableValue[]) => {
  if (isMode(value)) picked.value = value
}

const all = computed(() => (props.cut ? breakdownRows(props.cut) : []))
const rows = computed(() =>
  shownRows(all.value, mode.value, props.limit).map((row) => ({
    ...row,
    text: format.cellLabel(props.field, row.cell),
    icon: row.cell.t === 'string' ? valueIcon(props.field, row.cell.v) : null,
    flag: countryCode(props.field, row.cell),
  }))
)
const maxValue = computed(() => Math.max(1, ...rows.value.map((row) => row.value)))
const maxChange = computed(() => Math.max(1, ...rows.value.map((row) => Math.abs(row.change ?? 0))))

const signed = (change: number) => `${change > 0 ? '+' : change < 0 ? '−' : ''}${numbers.integer(Math.abs(change))}`
const tone = (row: { quiet: boolean; change: number | null }) =>
  row.quiet || row.change === null ? 'bg-fill-strong' : row.change > 0 ? 'bg-success' : 'bg-destructive'

const excludeCell = (cell: Cell) => {
  const next = exclude(props.state, props.field, cell)
  if (next) emit('commit', next)
}

const cellX = 'px-3 first:pl-4 last:pr-4 sm:px-4 sm:first:pl-4.5 sm:last:pr-4.5'
</script>

<template>
  <Panel
    :id="id"
    :title="title"
    :meta="cut && !views ? t(count, { count: cut.row_count }) : undefined"
    flush
  >
    <template
      v-if="views && view"
      #title
    >
      <h2
        :id="id"
        class="sr-only"
      >
        {{ title }}
      </h2>
      <ChoiceGroup
        :model-value="view"
        :label="viewsLabel ?? title"
        :options="views"
        @update:model-value="emit('view', $event)"
      />
    </template>
    <template #actions>
      <ToggleGroupRoot
        type="single"
        :model-value="mode"
        :aria-label="t('overview.panels.mode.label', { panel: title })"
        :class="[segmentedTrack, 'flex-nowrap']"
        @update:model-value="pick"
      >
        <ToggleGroupItem
          value="top"
          :class="segmentedItem()"
        >
          {{ t('overview.panels.mode.top') }}
        </ToggleGroupItem>
        <SimpleTooltip
          :tooltip="t('overview.panels.mode.needsComparison')"
          :disabled="comparable"
          as-child
        >
          <span class="inline-flex">
            <ToggleGroupItem
              value="movers"
              :disabled="!comparable"
              :aria-describedby="comparable ? undefined : `${id}-reason`"
              :class="segmentedItem()"
            >
              {{ t('overview.panels.mode.movers') }}
            </ToggleGroupItem>
          </span>
        </SimpleTooltip>
        <span
          v-if="!comparable"
          :id="`${id}-reason`"
          class="sr-only"
        >
          {{ t('overview.panels.mode.needsComparison') }}
        </span>
      </ToggleGroupRoot>
      <Button
        v-if="explorable"
        variant="ghost"
        size="toolbar"
        class="shrink-0 text-muted"
        :aria-label="t('explore.table.split', { field: format.fieldLabel(field) })"
        @click="emit('explore', field)"
      >
        <Icon
          name="lucide:arrow-up-right"
          aria-hidden="true"
        />
      </Button>
    </template>
    <table
      class="w-full table-fixed text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider"
      :aria-labelledby="id"
    >
      <colgroup v-if="mode === 'movers'">
        <col />
        <col class="w-18 sm:w-28" />
        <col class="w-12 sm:w-40" />
        <col class="w-14 sm:w-18" />
      </colgroup>
      <colgroup v-else>
        <col />
        <col class="w-22 sm:w-28" />
        <col class="w-15 sm:w-19" />
      </colgroup>
      <thead>
        <tr class="h-table-head bg-fill text-left text-[0.875rem] font-medium text-muted">
          <th
            scope="col"
            class="font-medium"
            :class="cellX"
          >
            <span class="flex min-w-0 items-center gap-2">
              <Icon
                v-if="headIcon"
                :name="headIcon"
                size="14"
                aria-hidden="true"
              />
              <span class="truncate">{{ format.fieldLabel(field) }}</span>
            </span>
          </th>
          <th
            scope="col"
            class="text-right font-medium"
            :class="cellX"
          >
            <span class="block truncate">{{ format.measureLabel('visitor_days') }}</span>
          </th>
          <th
            v-if="mode === 'movers'"
            scope="col"
            colspan="2"
            class="font-medium"
            :class="cellX"
          >
            <span class="flex min-w-0 items-center gap-2">
              <Icon
                name="lucide:chart-column"
                size="14"
                aria-hidden="true"
              />
              <span class="truncate">{{ t('overview.panels.change') }}</span>
            </span>
          </th>
          <th
            v-else
            scope="col"
            class="text-right font-medium"
            :class="cellX"
          >
            {{ t('overview.shareColumn') }}
          </th>
        </tr>
      </thead>
      <tbody v-if="cut">
        <tr
          v-for="row in rows"
          :key="canonical(row.cell)"
          class="group border-t border-divider tabular-nums"
          :class="['h-table-row', mode === 'movers' && row.quiet ? 'text-muted' : 'text-foreground']"
        >
          <th
            scope="row"
            class="text-left font-normal"
            :class="cellX"
          >
            <div class="relative -ml-2 flex min-w-0 items-center gap-2 px-2 py-1">
              <span
                v-if="mode === 'top' && !pips"
                class="absolute inset-y-0 left-0 rounded-md bg-series-primary/8 transition-[width] duration-300 ease-butter dark:bg-series-primary/10"
                :style="{ width: `${(row.value / maxValue) * 100}%` }"
                aria-hidden="true"
              />
              <span
                v-if="pips"
                class="relative flex shrink-0 gap-0.75"
                aria-hidden="true"
              >
                <i
                  v-for="pip in 5"
                  :key="pip"
                  class="h-3.5 w-1 rounded-[2px]"
                  :class="pip <= sharePips(row.share) ? 'bg-accent-text' : 'bg-accent-text/25'"
                />
              </span>
              <Icon
                v-if="row.icon"
                :name="row.icon"
                class="relative shrink-0 text-primary/80"
                aria-hidden="true"
              />
              <CountryFlag
                v-else-if="row.flag"
                :code="row.flag"
                class="relative"
              />
              <span
                class="relative truncate"
                :class="row.cell.t === 'not_set' ? 'text-muted italic' : ''"
                :title="row.text"
              >
                {{ row.text }}
              </span>
              <DropdownMenu>
                <DropdownMenuTrigger as-child>
                  <Button
                    variant="ghost"
                    size="toolbar"
                    class="relative -mr-1 ml-auto size-6 shrink-0 text-muted group-focus-within:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100"
                    :aria-label="t('explore.table.actionsFor', { value: row.text })"
                  >
                    <Icon
                      name="lucide:ellipsis"
                      aria-hidden="true"
                    />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start">
                  <DropdownMenuLabel class="max-w-64 truncate text-xs text-muted">{{ row.text }}</DropdownMenuLabel>
                  <DropdownMenuItem @select="emit('commit', include(state, field, row.cell))">
                    <Icon
                      name="lucide:filter"
                      aria-hidden="true"
                    />
                    {{ t('explore.table.include', { value: row.text }) }}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    :disabled="row.cell.t === 'null'"
                    @select="excludeCell(row.cell)"
                  >
                    <Icon
                      name="lucide:filter-x"
                      aria-hidden="true"
                    />
                    {{ t('explore.table.exclude', { value: row.text }) }}
                  </DropdownMenuItem>
                  <template v-if="explorable">
                    <DropdownMenuSeparator />
                    <DropdownMenuItem @select="emit('explore', field)">
                      <Icon
                        name="lucide:split"
                        aria-hidden="true"
                      />
                      {{ t('explore.table.split', { field: format.fieldLabel(field) }) }}
                    </DropdownMenuItem>
                  </template>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </th>
          <td
            class="text-right font-medium"
            :class="[cellX, mode === 'movers' && row.quiet ? '' : 'text-primary']"
          >
            {{ format.metric({ t: 'count', v: row.value }) }}
          </td>
          <template v-if="mode === 'movers'">
            <td
              :class="cellX"
              aria-hidden="true"
            >
              <span class="relative block h-2">
                <span class="absolute -inset-y-1.5 left-1/2 w-px bg-border" />
                <span
                  v-if="row.change"
                  class="absolute inset-y-0 rounded-[3px]"
                  :class="tone(row)"
                  :style="{
                    [row.change > 0 ? 'left' : 'right']: '50%',
                    width: `${(Math.abs(row.change) / maxChange) * 50}%`,
                  }"
                />
              </span>
            </td>
            <td
              class="text-right"
              :class="[
                cellX,
                row.quiet || row.change === null ? '' : row.change > 0 ? 'text-success' : 'text-destructive',
              ]"
            >
              {{ row.change === null ? '–' : signed(row.change) }}
            </td>
          </template>
          <td
            v-else
            class="text-right text-[0.875rem] text-muted"
            :class="cellX"
          >
            {{ numbers.percent(row.share) }}
          </td>
        </tr>
        <tr
          v-if="rows.length === 0"
          class="border-t border-divider"
        >
          <td
            :colspan="mode === 'movers' ? 4 : 3"
            class="px-4.5 py-6 text-center text-sm text-muted"
          >
            {{ t('explore.states.empty') }}
          </td>
        </tr>
      </tbody>
      <tbody v-else>
        <tr
          v-for="n in 4"
          :key="n"
          class="h-table-row border-t border-divider"
        >
          <td :class="cellX"><Skeleton class="h-4 w-3/4" /></td>
          <td :class="cellX"><Skeleton class="ml-auto h-4 w-10" /></td>
          <td :class="cellX">
            <Skeleton class="ml-auto h-3 w-8" />
          </td>
        </tr>
      </tbody>
    </table>
  </Panel>
</template>
