<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import DrillMenu from '~/components/report/DrillMenu.vue'
import PopulationNote from '~/components/report/PopulationNote.vue'
import ReportBars from '~/components/report/ReportBars.vue'
import ReportScreen from '~/components/report/ReportScreen.vue'
import StepEditor from '~/components/report/StepEditor.vue'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFilterFields } from '~/composables/useFilterFields'
import { useViewMode } from '~/composables/useViewMode'
import type { ExplorationState, IdentityScope, ReportResult, Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { complete, eventsFor, funnelOf, funnelState, sessionsFor, setFunnel } from '~/services/reports'

import FunnelStage from './FunnelStage.vue'

/**
 * Ordered steps within a window, per daily visitor or consented person (0003 Funnels). Two
 * views of one result: the ribbon with its leaks (default) and the step table; the choice is
 * remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const screen = useExplorationScreen(
  () => props.site.id,
  () => funnelState(props.site.id, props.site.timezone),
  () => [],
  complete
)
const { fields } = useFilterFields(() => props.site.id)

const view = useViewMode('funnels', ['ribbon', 'details'])
/** The flow view folds the step editor into one line until the person edits. */
const editing = ref(false)
const views = computed(() => [
  { value: 'ribbon' as const, label: t('reports.funnel.stage.view.ribbon') },
  { value: 'details' as const, label: t('reports.funnel.stage.view.details') },
])

const windows = [
  { value: 600, label: t('reports.funnel.windows.10m') },
  { value: 3600, label: t('reports.funnel.windows.1h') },
  { value: 86_400, label: t('reports.funnel.windows.1d') },
  { value: 604_800, label: t('reports.funnel.windows.7d') },
  { value: 2_592_000, label: t('reports.funnel.windows.30d') },
]
const scopes: { value: IdentityScope; label: string }[] = [
  { value: 'daily', label: t('reports.scope.daily') },
  { value: 'person', label: t('reports.scope.person') },
]

const funnel = computed(() => (screen.state.value ? funnelOf(screen.state.value) : null))
const commit = (patch: Parameters<typeof setFunnel>[1]) => {
  if (screen.state.value) void screen.commit(setFunnel(screen.state.value, patch))
}

const rows = (state: ExplorationState, report: ReportResult) => {
  if (report.kind !== 'funnel') return []
  const first = report.steps[0]?.reached ?? 0
  return report.steps.map((step, index) => ({
    key: String(index),
    label: t('reports.funnel.stepLabel', { step: index + 1 }),
    share: first > 0 ? step.reached / first : 0,
    values: [
      format.metric({ t: 'count', v: step.reached }),
      first > 0 ? format.metric({ t: 'ratio', v: step.reached / first, numerator: step.reached, denominator: first }) : '–',
      step.drop === undefined ? '' : format.metric({ t: 'count', v: step.drop }),
    ],
    step: funnelOf(state)?.steps[index],
  }))
}
</script>

<template>
  <ReportScreen
    :site="site"
    :screen="screen"
    :title="t('reports.funnel.title')"
  >
    <template #context>
      <ChoiceGroup
        v-model="view"
        :label="t('reports.funnel.stage.view.label')"
        :options="views"
      />
    </template>
    <template #builder="{ state }">
      <div
        v-if="view === 'ribbon' && !editing && funnel"
        class="flex flex-wrap items-center gap-x-4 gap-y-2"
      >
        <p class="text-sm text-muted">
          {{
            t('reports.funnel.stage.summary', {
              steps: funnel.steps.length,
              window: windows.find((option) => option.value === funnel?.window_seconds)?.label ?? '',
              scope: scopes.find((option) => option.value === funnel?.scope)?.label ?? '',
            })
          }}
        </p>
        <Button
          variant="outline"
          size="sm"
          @click="editing = true"
        >
          <Icon
            name="lucide:pencil"
            aria-hidden="true"
          />
          {{ t('reports.funnel.stage.edit') }}
        </Button>
      </div>
      <div
        v-else
        class="grid gap-4"
      >
        <StepEditor
          :state="state"
          :fields="fields"
          :refused="screen.refused.value"
          @commit="screen.commit"
        />
        <div
          v-if="funnel"
          class="flex flex-wrap items-center gap-x-6 gap-y-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm text-muted">{{ t('reports.funnel.window') }}</span>
            <ChoiceGroup
              :model-value="funnel.window_seconds"
              :label="t('reports.funnel.window')"
              :options="windows"
              @update:model-value="(value) => commit({ window_seconds: value })"
            />
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-sm text-muted">{{ t('reports.scope.label') }}</span>
            <ChoiceGroup
              :model-value="funnel.scope"
              :label="t('reports.scope.label')"
              :options="scopes"
              @update:model-value="(value) => commit({ scope: value })"
            />
          </div>
        </div>
      </div>
    </template>
    <template #result="{ state, result, report }">
      <FunnelStage
        v-if="report.kind === 'funnel' && view === 'ribbon'"
        :state="state"
        :report="report"
        :resolved="result.resolved"
      />
      <template v-else-if="report.kind === 'funnel'">
        <Panel
          :title="t('reports.funnel.conversion')"
          flush
        >
          <template #actions>
            <PopulationNote :notes="[{ scope: funnelOf(state)?.scope ?? 'daily', coverage: report.coverage }]" />
          </template>
          <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-4.5 pb-4">
            <p class="text-[2.375rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.metric(report.conversion) }}</p>
            <p class="text-sm text-muted tabular-nums">
              {{
                t('reports.funnel.conversionOf', {
                  last: format.metric({ t: 'count', v: report.steps.at(-1)?.reached ?? 0 }),
                  first: format.metric({ t: 'count', v: report.steps[0]?.reached ?? 0 }),
                })
              }}
            </p>
          </div>
          <ReportBars
            :caption="t('reports.funnel.table')"
            :columns="[t('reports.funnel.step'), t('reports.funnel.reached'), t('reports.funnel.fromFirst'), t('reports.funnel.drop')]"
            :rows="rows(state, report)"
          >
            <template #actions="{ row }">
              <DrillMenu
                v-if="row.step"
                :label="row.label"
                :sessions="sessionsFor(state, [row.step])"
                :events="eventsFor(state, [row.step])"
              />
            </template>
          </ReportBars>
        </Panel>
      </template>
    </template>
  </ReportScreen>
</template>
