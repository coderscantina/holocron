<script setup lang="ts">
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useStepDuration } from '~/composables/useStepDuration'
import type { Cell, Field, FunnelResult } from '~/generated'
import { GROUPS_SHOWN, splitRows } from '~/lib/funnel-split'
import { useI18n } from '~/plugins/i18n'
import { canonical } from '~/services/exploration'

/**
 * The table under a split funnel: all traffic first, then the top groups by visitors entered,
 * each with its reach per step as a bar against the all-traffic marker, its conversion and its
 * median time to convert. A row opens that group's own funnel; "Show all" lists every group the
 * server returned.
 */
const props = defineProps<{ result: FunnelResult; field: Field; picked: Cell | null }>()
const emit = defineEmits<{ pick: [cell: Cell | null] }>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()
const duration = useStepDuration()

const showAll = ref(false)
const split = computed(() => splitRows(props.result, showAll.value))
const fieldLabel = computed(() => format.fieldLabel(props.field))
const isPicked = (cell: Cell | null) => cell !== null && props.picked !== null && canonical(cell) === canonical(props.picked)
const label = (cell: Cell | null) => (cell ? format.cellLabel(props.field, cell) : t('reports.funnel.split.all'))
const percent = (ratio: number) => `${Math.max(0, Math.min(1, ratio)) * 100}%`
</script>

<template>
  <Panel
    :title="t('reports.funnel.split.title', { field: fieldLabel })"
    :meta="t('reports.funnel.split.count', { count: result.groups.length })"
    flush
  >
    <template
      v-if="result.groups_truncated"
      #actions
    >
      <span class="text-sm text-muted">{{ t('reports.funnel.split.truncated', { count: result.groups.length }) }}</span>
    </template>
    <div
      role="region"
      tabindex="0"
      :aria-label="t('reports.funnel.split.detailsCaption', { field: fieldLabel })"
      class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
    >
      <table class="w-full text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
        <caption class="sr-only">
          {{ t('reports.funnel.split.detailsCaption', { field: fieldLabel }) }}
        </caption>
        <thead>
          <tr class="h-table-head bg-fill text-[0.875rem] font-medium text-muted">
            <th
              scope="col"
              class="px-4 text-left font-medium whitespace-nowrap"
            >
              {{ fieldLabel }}
            </th>
            <th
              scope="col"
              class="px-4 text-right font-medium whitespace-nowrap"
            >
              {{ t('reports.funnel.split.entered') }}
            </th>
            <th
              scope="col"
              class="px-4 text-left font-medium whitespace-nowrap"
            >
              {{ t('reports.funnel.split.reach') }}
            </th>
            <th
              scope="col"
              class="px-4 text-right font-medium whitespace-nowrap"
            >
              {{ t('reports.funnel.split.conversion') }}
            </th>
            <th
              scope="col"
              class="px-4 text-right font-medium whitespace-nowrap"
            >
              {{ t('reports.funnel.split.toConvert') }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="row in split.rows"
            :key="row.key"
            class="h-table-row border-t border-divider"
            :class="isPicked(row.cell) ? 'bg-fill shadow-[inset_3px_0_0_var(--accent)]' : ''"
          >
            <th
              scope="row"
              class="min-w-40 px-2 py-1 text-left font-normal"
            >
              <button
                v-if="row.cell"
                type="button"
                class="block w-full max-w-[24rem] cursor-pointer truncate rounded-control px-2 py-1.5 text-left text-primary hover:bg-fill focus-visible:outline-2 focus-visible:outline-ring"
                :aria-pressed="isPicked(row.cell)"
                :aria-label="t('reports.funnel.split.pick', { group: label(row.cell) })"
                :title="label(row.cell)"
                @click="emit('pick', isPicked(row.cell) ? null : row.cell)"
              >
                {{ label(row.cell) }}
              </button>
              <button
                v-else
                type="button"
                class="block w-full cursor-pointer truncate rounded-control px-2 py-1.5 text-left text-muted hover:bg-fill focus-visible:outline-2 focus-visible:outline-ring"
                :aria-pressed="picked === null"
                @click="emit('pick', null)"
              >
                {{ label(null) }}
              </button>
            </th>
            <td class="px-4 py-2 text-right whitespace-nowrap text-foreground tabular-nums">{{ numbers.integer(row.entered) }}</td>
            <td class="min-w-36 px-4 py-2">
              <div class="grid gap-1">
                <span
                  v-for="(step, index) in row.reach"
                  :key="index"
                  class="relative block h-1 rounded-full bg-fill-strong"
                  role="img"
                  :aria-label="t('reports.funnel.split.reachStep', { step: index + 2, share: numbers.percent(step.share), overall: numbers.percent(step.overall) })"
                >
                  <span
                    class="block h-full rounded-full"
                    :class="isPicked(row.cell) ? 'bg-accent' : 'bg-series-comparison'"
                    :style="{ width: percent(step.top > 0 ? step.share / step.top : 0) }"
                  />
                  <span
                    class="absolute top-[-3px] h-[10px] w-px bg-primary"
                    :style="{ left: percent(step.top > 0 ? step.overall / step.top : 0) }"
                  />
                </span>
              </div>
            </td>
            <td class="px-4 py-2 text-right whitespace-nowrap text-foreground tabular-nums">{{ format.metric(row.conversion) }}</td>
            <td class="px-4 py-2 text-right whitespace-nowrap text-foreground tabular-nums">{{ duration.metric(row.median) ?? '–' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <div
      v-if="result.groups.length > GROUPS_SHOWN"
      class="border-t border-divider p-2"
    >
      <Button
        variant="ghost"
        size="sm"
        :aria-expanded="showAll"
        @click="showAll = !showAll"
      >
        {{ showAll ? t('reports.funnel.split.showTop', { count: GROUPS_SHOWN }) : t('reports.funnel.split.showAll', { count: result.groups.length }) }}
      </Button>
    </div>
  </Panel>
</template>
