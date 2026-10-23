<script setup lang="ts">
import SiteBoundary from '~/components/SiteBoundary.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import AttributionView from './AttributionView.vue'
import CampaignsView from './CampaignsView.vue'
import FunnelsView from './FunnelsView.vue'
import GoalsView from './GoalsView.vue'
import JourneysView from './JourneysView.vue'
import SessionsReport from './SessionsReport.vue'

/** The behavioral report routes of milestone 07, each a view inside the site boundary. */
const views = {
  funnels: { view: FunnelsView, title: 'reports.funnel.title' },
  goals: { view: GoalsView, title: 'reports.goals.title' },
  journeys: { view: JourneysView, title: 'reports.journey.title' },
  campaigns: { view: CampaignsView, title: 'reports.campaigns.title' },
  attribution: { view: AttributionView, title: 'reports.attribution.title' },
  sessions: { view: SessionsReport, title: 'reports.sessions.title' },
} as const satisfies Record<string, { view: Component; title: MessageKey }>

type ReportRoute = keyof typeof views
const isReportRoute = (name: unknown): name is ReportRoute => typeof name === 'string' && name in views

const route = useRoute()
const { t } = useI18n()

const siteId = computed(() => String(route.params.siteId))
const entry = computed(() => (isReportRoute(route.name) ? views[route.name] : views.funnels))
usePageMeta(() => t(entry.value.title))
</script>

<template>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <component
      :is="entry.view"
      :key="`${String(route.name)}:${site.id}`"
      :site="site"
    />
  </SiteBoundary>
</template>
