<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import DrillMenu from '~/components/report/DrillMenu.vue'
import PopulationNote from '~/components/report/PopulationNote.vue'
import ReportBars from '~/components/report/ReportBars.vue'
import ReportScreen from '~/components/report/ReportScreen.vue'
import { Panel } from '~/components/ui/panel'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useViewMode } from '~/composables/useViewMode'
import type { JourneyValue, PopulationScope, ReportResult, Site } from '~/generated'
import { REPORT_JOURNEY_LENGTH_MAX, REPORT_JOURNEY_LENGTH_MIN } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'
import { journeyOf, journeyState, sessionsFor, setJourney } from '~/services/reports'

import JourneysFlow from './JourneysFlow.vue'

/**
 * The paths sessions take, loops compressed, top 100 sequences (0003 Journeys). Two views of
 * the same result: the flow (default) with the builder on its canvas, and the details table
 * below the builder; the choice is remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const screen = useExplorationScreen(
  () => props.site.id,
  () => journeyState(props.site.id, props.site.timezone)
)

const journey = computed(() => (screen.state.value ? journeyOf(screen.state.value) : null))
const commit = (patch: Parameters<typeof setJourney>[1]) => {
  if (screen.state.value) void screen.commit(setJourney(screen.state.value, patch))
}
const values: { value: JourneyValue; label: string }[] = [
  { value: 'path', label: t('reports.journey.values.path') },
  { value: 'event_name', label: t('reports.journey.values.event_name') },
]
const scopes: { value: PopulationScope; label: string }[] = [
  { value: 'session', label: t('reports.scope.session') },
  { value: 'person', label: t('reports.scope.person') },
]
const lengths = Array.from({ length: REPORT_JOURNEY_LENGTH_MAX - REPORT_JOURNEY_LENGTH_MIN + 1 }, (_, i) => {
  const value = i + REPORT_JOURNEY_LENGTH_MIN
  return { value, label: String(value) }
})

const view = useViewMode('journeys', ['flow', 'details'])
const views = computed(() => [
  { value: 'flow' as const, label: t('reports.view.flow') },
  { value: 'details' as const, label: t('reports.view.details') },
])
const [DefineBuilder, Builder] = createReusableTemplate()

const rows = (report: ReportResult) => {
  if (report.kind !== 'journey') return []
  return report.sequences.map((sequence) => ({
    key: JSON.stringify(sequence.steps),
    label: sequence.steps.join(' → '),
    share: report.entry_sessions > 0 ? sequence.sessions / report.entry_sessions : 0,
    values: [
      format.metric({ t: 'count', v: sequence.sessions }),
      format.metric({
        t: 'ratio',
        v: report.entry_sessions > 0 ? sequence.sessions / report.entry_sessions : 0,
        numerator: sequence.sessions,
        denominator: report.entry_sessions,
      }),
      format.metric({ t: 'count', v: sequence.steps.length }),
    ],
  }))
}
</script>

<template>
  <DefineBuilder>
    <div
      v-if="journey"
      class="flex flex-wrap items-center gap-x-6 gap-y-3"
    >
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted">{{ t('reports.journey.value') }}</span>
        <ChoiceGroup
          :model-value="journey.value"
          :label="t('reports.journey.value')"
          :options="values"
          @update:model-value="(value) => commit({ value })"
        />
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted">{{ t('reports.journey.length') }}</span>
        <ChoiceGroup
          :model-value="journey.length"
          :label="t('reports.journey.length')"
          :options="lengths"
          @update:model-value="(length) => commit({ length })"
        />
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted">{{ t('reports.scope.label') }}</span>
        <ChoiceGroup
          :model-value="journey.scope"
          :label="t('reports.scope.label')"
          :options="scopes"
          @update:model-value="(scope) => commit({ scope })"
        />
      </div>
    </div>
  </DefineBuilder>
  <ReportScreen
    :site="site"
    :screen="screen"
    :title="t('reports.journey.title')"
  >
    <template #context>
      <ChoiceGroup
        v-model="view"
        :label="t('reports.view.label')"
        :options="views"
      />
    </template>
    <template
      v-if="view === 'details'"
      #builder
    >
      <Builder />
    </template>
    <template #result="{ state, report }">
      <JourneysFlow
        v-if="report.kind === 'journey' && view === 'flow'"
        :state="state"
        :report="report"
      >
        <template #toolbar>
          <Builder />
        </template>
      </JourneysFlow>
      <template v-else-if="report.kind === 'journey'">
        <Panel
          :title="t('reports.journey.table')"
          :meta="
            t('reports.journey.meta', {
              sessions: format.metric({ t: 'count', v: report.entry_sessions }),
              distinct: format.metric({ t: 'count', v: report.distinct_sequences }),
            })
          "
          flush
        >
          <template #actions>
            <DrillMenu
              :label="t('reports.journey.entering')"
              :sessions="sessionsFor(state)"
            />
            <PopulationNote :notes="[{ scope: journeyOf(state)?.scope === 'person' ? 'person' : 'session', coverage: report.coverage }]" />
          </template>
          <ReportBars
            :caption="t('reports.journey.table')"
            :columns="[t('reports.journey.sequence'), t('reports.journey.sessions'), t('reports.journey.share'), t('reports.journey.steps')]"
            :rows="rows(report)"
          />
        </Panel>
      </template>
    </template>
  </ReportScreen>
</template>
