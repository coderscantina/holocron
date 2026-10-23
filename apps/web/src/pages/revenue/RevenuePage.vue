<script setup lang="ts">
import SiteBoundary from '~/components/SiteBoundary.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

import RevenueView from './RevenueView.vue'

const route = useRoute()
const { t } = useI18n()

const siteId = computed(() => String(route.params.siteId))
usePageMeta(() => t('revenue.title'))
</script>

<template>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <RevenueView
      :key="site.id"
      :site="site"
    />
  </SiteBoundary>
</template>
