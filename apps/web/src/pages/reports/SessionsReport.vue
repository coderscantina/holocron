<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { useViewMode } from '~/composables/useViewMode'
import type { Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import SessionsTimeline from '../sessions/SessionsTimeline.vue'
import SessionsView from './SessionsView.vue'

/** The sessions timeline (default) or the paged session list; the choice is remembered in this browser. */
defineProps<{ site: Site }>()

const { t } = useI18n()
const view = useViewMode('sessions', ['timeline', 'details'])
const views = computed(() => [
  { value: 'timeline' as const, label: t('reports.sessions.view.timeline') },
  { value: 'details' as const, label: t('reports.sessions.view.details') },
])
const [DefineSwitch, ViewSwitch] = createReusableTemplate()
</script>

<template>
  <DefineSwitch>
    <ChoiceGroup
      v-model="view"
      :label="t('reports.sessions.view.label')"
      :options="views"
    />
  </DefineSwitch>
  <SessionsTimeline
    v-if="view === 'timeline'"
    :site="site"
  >
    <template #context>
      <ViewSwitch />
    </template>
  </SessionsTimeline>
  <SessionsView
    v-else
    :site="site"
  >
    <template #context>
      <ViewSwitch />
    </template>
  </SessionsView>
</template>
