<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFilterFields } from '~/composables/useFilterFields'
import { useViewMode } from '~/composables/useViewMode'
import type { DimensionField, ExplorationState, Panel, Site, TouchField } from '~/generated'
import { fieldLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { PAGE_SIZE_MAX } from '~/generated/limits'
import { canonical, defaultState, encodeState } from '~/services/exploration'
import { attributionState, setAttribution } from '~/services/reports'

import CampaignsFlow from './CampaignsFlow.vue'

/**
 * Campaign traffic: sessions, visitor-days and events split by a UTM field or click ID, the
 * session's touch values exactly as sent (0003 Attribution, UTM values). It is a plain
 * exploration, so drill-downs, comparison, saving and export work as in Explore. Two views:
 * the flow from channel to campaign value (default), from a panel of the same rows, and the
 * details table; the choice is remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()

const campaignFields = ['utm_campaign', 'utm_source', 'utm_medium', 'utm_content', 'utm_term', 'click_id'] as const
type CampaignField = (typeof campaignFields)[number]
const isCampaignField = (field: unknown): field is CampaignField => campaignFields.some((f) => f === field)

const defaults = (): ExplorationState => ({
  ...defaultState(props.site.id, props.site.timezone),
  range: { kind: 'relative', unit: 'day', count: 30 },
  measures: ['sessions', 'visitor_days', 'events'],
  dimensions: ['utm_campaign'],
  sort: [{ by: 'sessions', dir: 'desc' }],
  compare: 'none',
  visualization: 'table',
})
const fieldOf = (current: ExplorationState | null): CampaignField => {
  const first = current?.dimensions[0]
  return isCampaignField(first) ? first : 'utm_campaign'
}
/** Channel by the chosen field, for the flow; sorted by sessions, the most the server pages. */
const flowPanel = (current: ExplorationState | null): Panel[] => [
  {
    measures: ['sessions', 'revenue_net'],
    dimensions: ['channel', fieldOf(current)],
    sort: [{ by: 'sessions', dir: 'desc' }],
    page: { size: PAGE_SIZE_MAX, index: 0 },
  },
]
const screen = useExplorationScreen(() => props.site.id, defaults, flowPanel)
const state = screen.state
const result = screen.result
const { fields } = useFilterFields(() => props.site.id)

const field = computed<CampaignField>(() => fieldOf(state.value))
/** Replaces the first dimension; sort keys on the old field go with it. */
const choose = (next: CampaignField) => {
  const current = state.value
  if (!current) return
  const old: DimensionField = field.value
  const dimensions = [next, ...current.dimensions.slice(1).filter((d) => d !== next)]
  const sort = current.sort.filter((s) => canonical(s.by) !== canonical(old))
  void screen.commit({ ...current, dimensions, sort, page: { ...current.page, index: 0 } })
}
const options = campaignFields.map((value) => ({ value, label: t(fieldLabels[value]) }))

const attributionLink = computed(() => {
  const current = state.value
  if (!current) return null
  const touch: TouchField = field.value
  const next = { ...setAttribution(attributionState(props.site.id, current.timezone), { touch_field: touch }), range: current.range }
  return { name: 'attribution', params: { siteId: props.site.id }, query: { s: encodeState(next) } }
})
const view = useViewMode('campaigns', ['flow', 'details'])
const views = computed(() => [
  { value: 'flow' as const, label: t('reports.view.flow') },
  { value: 'details' as const, label: t('reports.view.details') },
])
const [DefineToolbar, Toolbar] = createReusableTemplate()
const empty = computed(() => !!result.value && result.value.main.row_count === 0)
</script>

<template>
  <ExplorationHeader
    :site="site"
    :state="state"
    :problem="screen.problem.value"
    :oversize="screen.oversize.value"
    :result="result"
    :fetching="screen.query.isFetching.value"
    :previous="screen.previous.value"
    :refusal="screen.refusal.value"
    :refused="screen.refused.value"
    :notice="screen.notice.value"
    :title="t('reports.campaigns.title')"
    :heading="{ title: t('reports.campaigns.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template
      v-if="state && !screen.refusal.value"
      #context
    >
      <ChoiceGroup
        v-model="view"
        :label="t('reports.view.label')"
        :options="views"
      />
    </template>
  </ExplorationHeader>

  <DefineToolbar>
    <div class="flex flex-wrap items-center gap-x-6 gap-y-3">
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-sm text-muted">{{ t('reports.campaigns.field') }}</span>
        <ChoiceGroup
          :model-value="field"
          :label="t('reports.campaigns.field')"
          :options="options"
          @update:model-value="choose"
        />
      </div>
      <Button
        v-if="attributionLink"
        variant="fill"
        size="sm"
        class="ml-auto"
        as-child
      >
        <RouterLink :to="attributionLink">
          <Icon
            name="lucide:git-merge"
            aria-hidden="true"
          />
          {{ t('reports.campaigns.attribute') }}
        </RouterLink>
      </Button>
    </div>
  </DefineToolbar>

  <template v-if="state && !screen.refusal.value && view === 'flow'">
    <CampaignsFlow
      v-if="result?.panels[0]"
      :state="state"
      :cut="result.panels[0]"
      :field="field"
    >
      <template #toolbar>
        <Toolbar />
      </template>
    </CampaignsFlow>
    <Skeleton
      v-else
      class="h-64 w-full"
    />
  </template>
  <template v-else-if="state && !screen.refusal.value">
    <div class="mb-4">
      <Toolbar />
    </div>
    <MeasureCards
      class="mb-4"
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
    />
    <EmptyState
      v-if="empty"
      icon="lucide:megaphone"
      :title="t('reports.campaigns.empty')"
      :description="t('reports.campaigns.emptyHint')"
      class="mb-4"
    />
    <ResultTable
      v-else-if="result"
      :state="state"
      :cut="result.main"
      :visitor-label="result.visitor_label"
      :split-fields="fields.map((f) => f.field)"
      @commit="screen.commit"
    />
    <Skeleton
      v-else
      class="h-64 w-full"
    />
  </template>
</template>
