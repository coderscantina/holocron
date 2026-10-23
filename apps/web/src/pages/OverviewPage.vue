<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSiteContext } from '~/composables/useSiteContext'
import { useViewMode } from '~/composables/useViewMode'
import { useI18n } from '~/plugins/i18n'

import OverviewPace from './overview/OverviewPace.vue'
import OverviewView from './overview/OverviewView.vue'

const route = useRoute()
const { t } = useI18n()
const context = useSiteContext()

const siteId = computed(() => String(route.params.siteId))
usePageMeta(() => context.site.value?.name ?? t('nav.overview'))

/** Today's pace (default) or the full overview with ranges and filters; remembered in this browser. */
const view = useViewMode('overview', ['today', 'details'])
const views = computed(() => [
  { value: 'today' as const, label: t('overview.view.today') },
  { value: 'details' as const, label: t('overview.view.details') },
])
const [DefineSwitch, ViewSwitch] = createReusableTemplate()
</script>

<template>
  <DefineSwitch>
    <ChoiceGroup
      v-model="view"
      :label="t('overview.view.label')"
      :options="views"
    />
  </DefineSwitch>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <OverviewPace
      v-if="view === 'today'"
      :key="site.id"
      :site="site"
    >
      <template #context>
        <ViewSwitch />
      </template>
    </OverviewPace>
    <OverviewView
      v-else
      :key="site.id"
      :site="site"
    >
      <template #context>
        <ViewSwitch />
      </template>
    </OverviewView>
  </SiteBoundary>
</template>
