<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import DrillMenu from '~/components/report/DrillMenu.vue'
import PopulationNote from '~/components/report/PopulationNote.vue'
import ReportBars from '~/components/report/ReportBars.vue'
import ReportScreen from '~/components/report/ReportScreen.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Panel } from '~/components/ui/panel'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useExplore } from '~/composables/useExplore'
import { useGoals } from '~/composables/useGoals'
import { usePredicateText } from '~/composables/usePredicateText'
import { useViewMode } from '~/composables/useViewMode'
import type {
  AttributedConversion,
  AttributionModel,
  Conversion,
  ExplorationState,
  IdentityScope,
  Site,
  TouchField,
} from '~/generated'
import { REPORT_ATTRIBUTION_CONVERSIONS_MAX } from '~/generated/limits'
import { fieldLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import {
  addConversion,
  attributionOf,
  attributionState,
  complete,
  fieldEquals,
  inline,
  removeConversion,
  replaceConversionGoal,
  sessionsFor,
  setAttribution,
  upgradeFor,
} from '~/services/reports'

import AttributionFlow from './AttributionFlow.vue'

/**
 * Which touch gets the credit for goal completions and revenue: the last or first non-direct
 * touch before the conversion, within 30 days, internal referrers ignored (0003 Attribution).
 * Revenue stays per currency. Two views: the flow from touch to conversion (default), with the
 * same report under the other model beside it, and the details tables; the choice is
 * remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const { text } = usePredicateText()
const goals = useGoals(() => props.site.id)
const screen = useExplorationScreen(
  () => props.site.id,
  () => attributionState(props.site.id, props.site.timezone),
  () => [],
  complete
)

const attribution = computed(() => (screen.state.value ? attributionOf(screen.state.value) : null))
const commit = (next: ExplorationState) => void screen.commit(next)

const models: { value: AttributionModel; label: string }[] = [
  { value: 'last_touch', label: t('reports.attribution.models.last_touch') },
  { value: 'first_touch', label: t('reports.attribution.models.first_touch') },
]
const scopes: { value: IdentityScope; label: string }[] = [
  { value: 'daily', label: t('reports.scope.daily') },
  { value: 'person', label: t('reports.scope.person') },
]
const touchFields: TouchField[] = [
  'channel',
  'referrer_domain',
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_content',
  'utm_term',
  'click_id',
]
const isTouchField = (value: string): value is TouchField => touchFields.some((f) => f === value)
const onTouchField = (value: unknown) => {
  if (screen.state.value && typeof value === 'string' && isTouchField(value)) {
    commit(setAttribution(screen.state.value, { touch_field: value }))
  }
}
const fieldId = useId()
const coverage = computed(() => {
  const report = screen.result.value?.report
  return report?.kind === 'attribution' ? report.coverage : null
})

const conversionName = (conversion: Conversion) => {
  if (conversion.kind === 'revenue') return t('reports.attribution.revenue')
  const saved = goals.goals.value.find((g) => g.id === conversion.goal.ref?.id)
  return saved ? saved.name : text(conversion.goal.predicate)
}
const upgradeOf = (conversion: Conversion) =>
  conversion.kind === 'goal' ? upgradeFor(conversion.goal, goals.goals.value) : null

const view = useViewMode('attribution', ['flow', 'details'])
const views = computed(() => [
  { value: 'flow' as const, label: t('reports.view.flow') },
  { value: 'details' as const, label: t('reports.view.details') },
])
const [DefineBuilder, Builder] = createReusableTemplate<{ state: ExplorationState }>()

/** The same report under the other model, for the flow's comparison. */
const otherState = computed(() => {
  const state = screen.state.value
  const current = state ? attributionOf(state) : null
  if (view.value !== 'flow' || !state || !current || !complete(state)) return null
  return setAttribution(state, { model: current.model === 'last_touch' ? 'first_touch' : 'last_touch' })
})
const otherQuery = useExplore(otherState).query
const other = computed(() => {
  const report = otherQuery.data.value?.report
  return report?.kind === 'attribution' && !otherQuery.isPlaceholderData.value ? report : null
})

const upgrade = (state: ExplorationState, index: number, conversion: Conversion) => {
  const next = upgradeOf(conversion)
  if (next) commit(replaceConversionGoal(state, index, inline(next)))
}

const rows = (state: ExplorationState, conversion: Conversion, result: AttributedConversion) => {
  const total = result.direct.conversions + result.touches.reduce((sum, row) => sum + row.attributed.conversions, 0)
  const field = attributionOf(state)?.touch_field ?? 'channel'
  const revenue = conversion.kind === 'revenue'
  const values = (conversions: number, money: AttributedConversion['direct']['revenue']) => [
    format.metric({ t: 'count', v: conversions }),
    total > 0 ? format.metric({ t: 'ratio', v: conversions / total, numerator: conversions, denominator: total }) : '–',
    ...(revenue ? [format.metric({ t: 'money', v: money })] : []),
  ]
  return [
    ...result.touches.map((row) => {
      const value = row.touch.t === 'string' ? row.touch.v : null
      const label = value ?? t('explore.table.notSet')
      return {
        key: label,
        label,
        muted: value === null,
        share: total > 0 ? row.attributed.conversions / total : 0,
        values: values(row.attributed.conversions, row.attributed.revenue),
        sessions: sessionsFor(state, [fieldEquals(state, field, value)]),
      }
    }),
    {
      key: '(direct)',
      label: t('reports.attribution.direct'),
      muted: true,
      share: total > 0 ? result.direct.conversions / total : 0,
      values: values(result.direct.conversions, result.direct.revenue),
      sessions: undefined,
    },
  ]
}
</script>

<template>
  <DefineBuilder v-slot="{ state }">
    <div
      v-if="attribution"
      class="grid gap-4"
    >
      <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div class="flex items-center gap-2">
          <span class="text-sm text-muted">{{ t('reports.attribution.model') }}</span>
          <ChoiceGroup
            :model-value="attribution.model"
            :label="t('reports.attribution.model')"
            :options="models"
            @update:model-value="(model) => commit(setAttribution(state, { model }))"
          />
          <SimpleTooltip
            :tooltip="t('reports.attribution.lookback', { days: attribution.lookback_days })"
            :aria-label="t('reports.attribution.lookback', { days: attribution.lookback_days })"
            class="inline-flex size-7 items-center justify-center rounded-md text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-ring"
          >
            <Icon
              name="lucide:info"
              aria-hidden="true"
            />
          </SimpleTooltip>
        </div>
        <div class="flex items-center gap-2">
          <span class="text-sm text-muted">{{ t('reports.scope.label') }}</span>
          <ChoiceGroup
            :model-value="attribution.scope"
            :label="t('reports.scope.label')"
            :options="scopes"
            @update:model-value="(scope) => commit(setAttribution(state, { scope }))"
          />
          <PopulationNote
            v-if="coverage"
            :notes="[{ scope: attribution.scope, coverage }]"
          />
        </div>
        <div class="flex items-center gap-2">
          <label
            :for="fieldId"
            class="text-sm text-muted"
          >
            {{ t('reports.attribution.touchField') }}
          </label>
          <Select
            :model-value="attribution.touch_field"
            @update:model-value="onTouchField"
          >
            <SelectTrigger
              :id="fieldId"
              class="h-8 w-44"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="field in touchFields"
                :key="field"
                :value="field"
              >
                {{ t(fieldLabels[field]) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted">{{ t('reports.attribution.conversions') }}</span>
        <span
          v-for="(conversion, index) in attribution.conversions"
          :key="index"
          class="inline-flex max-w-full items-center gap-1 rounded-md bg-fill py-0.5 pr-0.5 pl-2 text-sm text-primary"
        >
          <span class="truncate">{{ conversionName(conversion) }}</span>
          <Button
            v-if="upgradeOf(conversion)"
            variant="ghost"
            size="toolbar"
            :aria-label="t('reports.goals.upgradeNamed', { name: conversionName(conversion), version: upgradeOf(conversion)?.version ?? 0 })"
            @click="upgrade(state, index, conversion)"
          >
            <Icon name="lucide:circle-arrow-up" />
          </Button>
          <Button
            variant="ghost"
            size="toolbar"
            :disabled="attribution.conversions.length <= 1"
            :aria-label="t('reports.attribution.removeConversion', { name: conversionName(conversion) })"
            @click="commit(removeConversion(state, index))"
          >
            <Icon name="lucide:x" />
          </Button>
        </span>
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button
              variant="fill"
              size="xs"
              :disabled="attribution.conversions.length >= REPORT_ATTRIBUTION_CONVERSIONS_MAX"
            >
              <Icon
                name="lucide:plus"
                aria-hidden="true"
              />
              {{ t('reports.attribution.addConversion') }}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            class="w-72"
          >
            <DropdownMenuItem @select="commit(addConversion(state, { kind: 'revenue' }))">
              {{ t('reports.attribution.revenue') }}
            </DropdownMenuItem>
            <DropdownMenuSeparator v-if="goals.goals.value.length > 0" />
            <DropdownMenuItem
              v-for="goal in goals.goals.value"
              :key="goal.id"
              @select="commit(addConversion(state, { kind: 'goal', goal: inline(goal) }))"
            >
              <span class="truncate">{{ goal.name }}</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  </DefineBuilder>
  <ReportScreen
    :site="site"
    :screen="screen"
    :title="t('reports.attribution.title')"
  >
    <template #context>
      <ChoiceGroup
        v-model="view"
        :label="t('reports.view.label')"
        :options="views"
      />
    </template>
    <template
      v-if="view === 'details' || (screen.state.value && !complete(screen.state.value))"
      #builder="{ state }"
    >
      <Builder :state="state" />
    </template>
    <template #result="{ state, report }">
      <AttributionFlow
        v-if="report.kind === 'attribution' && attribution && view === 'flow'"
        :state="state"
        :report="report"
        :other="other"
        :conversions="attribution.conversions"
        :names="attribution.conversions.map(conversionName)"
      >
        <template #toolbar>
          <Builder :state="state" />
        </template>
      </AttributionFlow>
      <template v-else-if="report.kind === 'attribution' && attribution">
        <template
          v-for="(result, index) in report.conversions"
          :key="index"
        >
          <Panel
            v-if="attribution.conversions[index]"
            :title="conversionName(attribution.conversions[index])"
            :meta="
              result.touch_count > result.touches.length
                ? t('reports.attribution.top', { shown: result.touches.length, count: result.touch_count })
                : undefined
            "
            flush
            class="mb-4"
          >
            <ReportBars
              :caption="t('reports.attribution.table', { name: conversionName(attribution.conversions[index]) })"
              :columns="[
                t(fieldLabels[attribution.touch_field]),
                t('reports.attribution.conversionsColumn'),
                t('reports.attribution.share'),
                ...(attribution.conversions[index].kind === 'revenue' ? [t('reports.attribution.netRevenue')] : []),
              ]"
              :rows="rows(state, attribution.conversions[index], result)"
            >
              <template #actions="{ row }">
                <DrillMenu
                  v-if="row.sessions"
                  :label="row.label"
                  :sessions="row.sessions"
                />
              </template>
            </ReportBars>
          </Panel>
        </template>
      </template>
    </template>
  </ReportScreen>
</template>
