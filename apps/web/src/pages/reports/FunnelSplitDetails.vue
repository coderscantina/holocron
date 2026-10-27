<script setup lang="ts">
import DrillMenu from '~/components/report/DrillMenu.vue'
import { Button } from '~/components/ui/button'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useStepDuration } from '~/composables/useStepDuration'
import type { ExplorationState, Field, FunnelResult } from '~/generated'
import { GROUPS_SHOWN, splitRows } from '~/lib/funnel-split'
import { useI18n } from '~/plugins/i18n'
import { eventsFor, funnelOf, pickGroup, sessionsFor } from '~/services/reports'

/**
 * A split funnel as one table: a row per group, all traffic first, with the visitors who reached
 * each step and their share of step 1, then conversion and median time to convert. A step cell
 * opens the sessions or events behind it, for that group.
 */
const props = defineProps<{ state: ExplorationState; result: FunnelResult; field: Field }>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()
const duration = useStepDuration()

const showAll = ref(false)
const split = computed(() => splitRows(props.result, showAll.value))
const steps = computed(() => funnelOf(props.state)?.steps ?? [])
const fieldLabel = computed(() => format.fieldLabel(props.field))

/** The state a row's drill menus read: all traffic as is, a group as its own filtered funnel. */
const scope = (cell: (typeof split.value.rows)[number]['cell']) => (cell ? pickGroup(props.state, cell) : props.state)
const share = (reached: number, entered: number) => (entered > 0 ? numbers.percent(reached / entered) : '–')
const label = (cell: (typeof split.value.rows)[number]['cell']) => (cell ? format.cellLabel(props.field, cell) : t('reports.funnel.split.all'))
</script>

<template>
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
            v-for="(_, index) in steps"
            :key="index"
            scope="col"
            class="px-4 text-right font-medium whitespace-nowrap"
          >
            {{ t('reports.funnel.stepLabel', { step: index + 1 }) }}
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
        >
          <th
            scope="row"
            class="min-w-40 px-4 py-2 text-left font-normal"
          >
            <span
              class="block max-w-[24rem] truncate"
              :class="row.cell ? 'text-primary' : 'text-muted'"
              :title="label(row.cell)"
            >
              {{ label(row.cell) }}
            </span>
          </th>
          <td
            v-for="(reached, index) in row.steps"
            :key="index"
            class="px-4 py-1 text-right whitespace-nowrap tabular-nums"
          >
            <span class="inline-flex items-center justify-end gap-1">
              <span class="text-foreground">
                {{ numbers.integer(reached) }}
                <span
                  v-if="index > 0"
                  class="text-muted"
                >
                  {{ share(reached, row.entered) }}
                </span>
              </span>
              <DrillMenu
                v-if="steps[index]"
                :label="`${label(row.cell)}, ${t('reports.funnel.stepLabel', { step: index + 1 })}`"
                :sessions="sessionsFor(scope(row.cell), [steps[index]!])"
                :events="eventsFor(scope(row.cell), [steps[index]!])"
              />
            </span>
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
</template>
