<script setup lang="ts">
import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Cell, Cut, ExplorationState, Field, Measure, Sort, VisitorLabel } from '~/generated'
import { countryCode } from '~/lib/client-looks'
import { useI18n } from '~/plugins/i18n'
import { canSplit, canonical, exclude, include, setPage, splitBy, toggleSort } from '~/services/exploration'

import ChangeBadge from './ChangeBadge.vue'

/**
 * The rows of a cut as a semantic table: dimension cells with named include, exclude and
 * split-by actions, measures with their change, sortable headers, the totals row and paging.
 * It scrolls inside its own labelled region on narrow screens.
 */
const props = defineProps<{
  state: ExplorationState
  cut: Cut
  visitorLabel: VisitorLabel | null
  /** Dimensions the split-by action offers. */
  splitFields: Field[]
  /** Dimensions every shown row shares and the page names elsewhere, left out of the columns. */
  implied?: Field[]
}>()

const emit = defineEmits<{ commit: [state: ExplorationState] }>()
defineSlots<{
  /** More menu items for one cell, after include and exclude. */
  'cell-actions'?: (props: { field: Field; cell: Cell }) => unknown
}>()

const { t } = useI18n()
const format = useExplorationFormat()

const measureLabel = (measure: Measure) =>
  (measure === 'visitor_days' || measure === 'visitors') && props.visitorLabel
    ? format.measureLabel(props.visitorLabel)
    : format.measureLabel(measure)

const sortOf = (key: Sort['by']) => props.state.sort.find((s) => canonical(s.by) === canonical(key))
const ariaSort = (key: Sort['by']) => {
  const sort = sortOf(key)
  return sort ? (sort.dir === 'asc' ? 'ascending' : 'descending') : 'none'
}

const pages = computed(() => Math.max(1, Math.ceil(props.cut.row_count / props.state.page.size)))
const page = computed(() => props.state.page.index + 1)
const regionId = useId()

const isImplied = (field: Field) => (props.implied ?? []).some((other) => canonical(other) === canonical(field))
/** The dimensions shown as columns. */
const columns = computed(() => props.state.dimensions.filter((field) => !isImplied(field)))

/** A row's shown cells with their dimension and display text. */
const cellsOf = (key: Cell[]) =>
  key.flatMap((cell, index) => {
    const field = props.state.dimensions[index]
    return field && !isImplied(field)
      ? [{ field, cell, text: format.cellLabel(field, cell), flag: countryCode(field, cell) }]
      : []
  })
const rowName = (key: Cell[]) =>
  cellsOf(key)
    .map((entry) => entry.text)
    .join(' · ')

const excludeCell = (field: Field, cell: Cell) => {
  const next = exclude(props.state, field, cell)
  if (next) emit('commit', next)
}
const splitChoices = computed(() => props.splitFields.filter((field) => canSplit(props.state, field)))
</script>

<template>
  <div class="overflow-hidden rounded-panel border border-hairline bg-card">
    <div
      :id="regionId"
      role="region"
      tabindex="0"
      :aria-label="t('explore.table.label')"
      class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
    >
      <table class="w-full text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
        <caption class="sr-only">{{ t('explore.table.label') }}</caption>
        <thead class="bg-fill text-[0.875rem] text-muted">
          <tr>
            <th
              v-for="(field, index) in columns"
              :key="`d${index}`"
              scope="col"
              class="h-table-head px-4 text-left font-medium whitespace-nowrap"
              :aria-sort="ariaSort(field)"
            >
              <button
                type="button"
                class="-mx-1 inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                :aria-label="t('explore.table.sortBy', { column: format.fieldLabel(field) })"
                @click="emit('commit', toggleSort(state, field))"
              >
                {{ format.fieldLabel(field) }}
                <Icon
                  :name="sortOf(field)?.dir === 'asc' ? 'lucide:arrow-up' : 'lucide:arrow-down'"
                  size="12"
                  :class="sortOf(field) ? 'opacity-100' : 'opacity-30'"
                  aria-hidden="true"
                />
              </button>
            </th>
            <th
              v-for="(measure, index) in state.measures"
              :key="`m${index}`"
              scope="col"
              class="h-table-head px-4 text-right font-medium whitespace-nowrap"
              :aria-sort="ariaSort(measure)"
            >
              <button
                type="button"
                class="-mx-1 inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                :aria-label="t('explore.table.sortBy', { column: measureLabel(measure) })"
                @click="emit('commit', toggleSort(state, measure))"
              >
                {{ measureLabel(measure) }}
                <Icon
                  :name="sortOf(measure)?.dir === 'asc' ? 'lucide:arrow-up' : 'lucide:arrow-down'"
                  size="12"
                  :class="sortOf(measure) ? 'opacity-100' : 'opacity-30'"
                  aria-hidden="true"
                />
              </button>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in cut.rows"
            :key="canonical(row.key)"
            class="group h-table-row border-t border-divider"
          >
            <th
              v-for="entry in cellsOf(row.key)"
              :key="canonical(entry.field)"
              scope="row"
              class="max-w-72 px-4 py-1 text-left font-normal text-primary"
            >
              <div class="flex min-w-0 items-center gap-1">
                <CountryFlag
                  v-if="entry.flag"
                  :code="entry.flag"
                  class="mr-1"
                />
                <span
                  class="truncate"
                  :class="entry.cell.t === 'not_set' || entry.cell.t === 'null' ? 'text-muted italic' : ''"
                  :title="entry.text"
                >
                  {{ entry.text }}
                </span>
                <DropdownMenu>
                  <DropdownMenuTrigger as-child>
                    <Button
                      variant="ghost"
                      size="toolbar"
                      class="size-6 shrink-0 text-muted group-focus-within:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 pointer-fine:opacity-0 pointer-fine:group-hover:opacity-100"
                      :aria-label="t('explore.table.actionsFor', { value: rowName(row.key) })"
                    >
                      <Icon
                        name="lucide:ellipsis"
                        aria-hidden="true"
                      />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="start">
                    <DropdownMenuLabel class="max-w-64 truncate text-xs text-muted">
                      {{ entry.text }}
                    </DropdownMenuLabel>
                    <DropdownMenuItem @select="emit('commit', include(state, entry.field, entry.cell))">
                      <Icon
                        name="lucide:filter"
                        aria-hidden="true"
                      />
                      {{ t('explore.table.include', { value: entry.text }) }}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      :disabled="entry.cell.t === 'null'"
                      @select="excludeCell(entry.field, entry.cell)"
                    >
                      <Icon
                        name="lucide:filter-x"
                        aria-hidden="true"
                      />
                      {{ t('explore.table.exclude', { value: entry.text }) }}
                    </DropdownMenuItem>
                    <slot
                      name="cell-actions"
                      :field="entry.field"
                      :cell="entry.cell"
                    />
                    <template v-if="splitChoices.length > 0">
                      <DropdownMenuSeparator />
                      <DropdownMenuSub>
                        <DropdownMenuSubTrigger>
                          <Icon
                            name="lucide:split"
                            aria-hidden="true"
                          />
                          {{ t('explore.dimensionsPicker') }}
                        </DropdownMenuSubTrigger>
                        <DropdownMenuSubContent class="max-h-72 overflow-y-auto">
                          <DropdownMenuItem
                            v-for="field in splitChoices"
                            :key="canonical(field)"
                            @select="emit('commit', splitBy(state, field))"
                          >
                            {{ t('explore.table.split', { field: format.fieldLabel(field) }) }}
                          </DropdownMenuItem>
                        </DropdownMenuSubContent>
                      </DropdownMenuSub>
                    </template>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </th>
            <td
              v-for="(value, index) in row.values"
              :key="index"
              class="px-4 py-1 text-right whitespace-nowrap tabular-nums"
            >
              <span class="font-medium text-primary">{{ format.metric(value) }}</span>
              <span
                v-if="row.compare?.[index]"
                class="ml-2 text-[0.875rem] text-muted"
              >
                <ChangeBadge
                  :comparison="row.compare[index]"
                  :measure="state.measures[index]"
                />
              </span>
            </td>
          </tr>
        </tbody>
        <tfoot class="border-t border-divider bg-fill">
          <tr>
            <th
              scope="row"
              :colspan="Math.max(1, columns.length)"
              class="h-table-row px-4 py-1 text-left text-[0.875rem] font-medium text-muted"
            >
              {{ t('explore.table.total') }}
            </th>
            <td
              v-for="(value, index) in cut.totals"
              :key="index"
              class="px-4 py-1 text-right whitespace-nowrap tabular-nums"
            >
              <span class="font-semibold text-primary">{{ format.metric(value) }}</span>
              <span
                v-if="cut.compare?.[index]"
                class="ml-2 text-[0.875rem] text-muted"
              >
                <ChangeBadge
                  :comparison="cut.compare[index]"
                  :measure="state.measures[index]"
                />
              </span>
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
    <div
      v-if="pages > 1"
      class="flex items-center justify-end gap-2 border-t border-divider px-4 py-2 text-xs text-muted"
    >
      <Button
        variant="outline"
        size="xs"
        :disabled="page <= 1"
        :aria-label="t('explore.table.previousPage')"
        @click="emit('commit', setPage(state, state.page.index - 1))"
      >
        <Icon name="lucide:chevron-left" />
      </Button>
      <span class="tabular-nums">{{ t('explore.table.pageOf', { page, pages }) }}</span>
      <Button
        variant="outline"
        size="xs"
        :disabled="page >= pages"
        :aria-label="t('explore.table.nextPage')"
        @click="emit('commit', setPage(state, state.page.index + 1))"
      >
        <Icon name="lucide:chevron-right" />
      </Button>
    </div>
  </div>
</template>
