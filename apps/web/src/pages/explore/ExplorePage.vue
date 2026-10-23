<script setup lang="ts">
import SiteBoundary from '~/components/SiteBoundary.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

import ExploreView from './ExploreView.vue'

const route = useRoute()
const { t } = useI18n()

const siteId = computed(() => String(route.params.siteId))
usePageMeta(() => t('explore.title'))
</script>

<template>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <ExploreView
      :key="site.id"
      :site="site"
    />
  </SiteBoundary>
</template>
