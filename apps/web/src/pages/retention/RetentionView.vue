<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import CohortTable from '~/components/retention/CohortTable.vue'
import RetentionLabels from '~/components/retention/RetentionLabels.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useIdentityPolicy } from '~/composables/useSitePolicies'
import { useViewMode } from '~/composables/useViewMode'
import type { Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import {
  cohortPeriod,
  intervalChoices,
  intervalsOf,
  retentionState,
  setCohortPeriod,
  setIntervals,
  type CohortPeriod,
} from '~/services/retention'

import RetentionHeatmapView from './RetentionHeatmapView.vue'

/**
 * Retention of consented persistent profiles by daily or weekly cohort (0003 Retention and
 * cohorts). Two views of the same result: the heatmap (default) and the details table; the
 * choice is remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()

const screen = useExplorationScreen(
  () => props.site.id,
  () => retentionState(props.site.id, props.site.timezone)
)
const state = screen.state
const result = screen.result
const retention = computed(() => (result.value?.report?.kind === 'retention' ? result.value.report : null))

const canConfigure = computed(() => props.site.capabilities.includes('site.configure'))
const policy = useIdentityPolicy(() => props.site.id, canConfigure)
const persistentOff = computed(() => policy.data.value?.persistent_identity === false)

/** Nothing consented: daily pseudonyms cannot show that a person came back. */
const noConsent = computed(() => retention.value?.coverage.with_identity === 0)
/** Consented traffic, but no profile had its first event in these periods. */
const noEntries = computed(
  () => !!retention.value && !noConsent.value && retention.value.cohorts.every((cohort) => cohort.size === 0)
)

const periods = computed<{ value: CohortPeriod; label: string }[]>(() => [
  { value: 'day', label: t('retention.period.day') },
  { value: 'week', label: t('retention.period.week') },
])
const period = computed(() => (state.value ? cohortPeriod(state.value) : 'week'))
const intervals = computed(() => (state.value ? intervalsOf(state.value) : 0))
const onPeriod = (value: CohortPeriod) => {
  if (state.value) void screen.commit(setCohortPeriod(state.value, value))
}
const view = useViewMode('retention', ['heatmap', 'details'])
const views = computed(() => [
  { value: 'heatmap' as const, label: t('retention.view.heatmap') },
  { value: 'details' as const, label: t('retention.view.details') },
])

/** The period and interval controls sit above the details, and in the heatmap's own header when it shows. */
const [DefineControls, ReuseControls] = createReusableTemplate()
const heatmapShown = computed(() => view.value === 'heatmap' && !!retention.value && !noConsent.value && !noEntries.value)

const intervalsLabel = computed(() =>
  t(period.value === 'day' ? 'retention.intervals.days' : 'retention.intervals.weeks', { count: intervals.value })
)
const onIntervals = (value: unknown) => {
  if (state.value && typeof value === 'number') void screen.commit(setIntervals(state.value, value))
}
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
    :title="t('retention.title')"
    :heading="{ title: t('retention.title'), description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template #context>
      <ChoiceGroup
        v-model="view"
        :label="t('retention.view.label')"
        :options="views"
      />
    </template>
  </ExplorationHeader>

  <DefineControls>
    <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
      <ChoiceGroup
        :model-value="period"
        :label="t('retention.period.label')"
        :options="periods"
        @update:model-value="onPeriod"
      />
      <DropdownMenu>
        <DropdownMenuTrigger as-child>
          <Button
            variant="outline"
            size="sm"
            :aria-label="t('retention.intervals.label', { value: intervalsLabel })"
          >
            <Icon
              name="lucide:columns-3"
              aria-hidden="true"
            />
            {{ t(period === 'day' ? 'retention.intervals.days' : 'retention.intervals.weeks', { count: intervals }) }}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuRadioGroup
            :model-value="intervals"
            @update:model-value="onIntervals"
          >
            <DropdownMenuRadioItem
              v-for="choice in intervalChoices[period]"
              :key="choice"
              :value="choice"
            >
              {{ t(period === 'day' ? 'retention.intervals.days' : 'retention.intervals.weeks', { count: choice }) }}
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </DefineControls>

  <template v-if="state && !screen.refusal.value">
    <div
      v-if="!heatmapShown"
      class="mb-4"
    >
      <ReuseControls />
    </div>

    <template v-if="retention && result">
      <RetentionLabels
        v-if="view === 'details'"
        class="mb-4"
        :result="retention"
        :timezone="result.resolved.timezone"
      />
      <EmptyState
        v-if="noConsent"
        icon="lucide:user-x"
        :title="t('retention.empty.noConsent')"
        :description="t('retention.empty.noConsentHint')"
      >
        <RouterLink
          v-if="persistentOff"
          :to="{ name: 'site-settings-identity', params: { siteId: site.id } }"
          class="text-sm font-semibold text-accent-text underline-offset-2 hover:underline"
        >
          {{ t('retention.empty.enable') }}
        </RouterLink>
      </EmptyState>
      <EmptyState
        v-else-if="noEntries"
        icon="lucide:calendar-x"
        :title="t('retention.empty.noEntries')"
        :description="t('retention.empty.noEntriesHint')"
      />
      <RetentionHeatmapView
        v-else-if="view === 'heatmap'"
        :site="site"
        :state="state"
        :result="retention"
        :timezone="result.resolved.timezone"
      >
        <template #toolbar>
          <ReuseControls />
        </template>
      </RetentionHeatmapView>
      <CohortTable
        v-else
        :result="retention"
        :granularity="state.granularity"
        :timezone="result.resolved.timezone"
      />
    </template>
    <Skeleton
      v-else
      class="h-72 w-full"
    />
  </template>
</template>
