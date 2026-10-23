<script setup lang="ts">
import PageHeader from '~/components/PageHeader.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n, type MessageKey } from '~/plugins/i18n'

const route = useRoute()
const { t } = useI18n()

const siteId = computed(() => String(route.params.siteId))
usePageMeta(() => t('settings.title'))

const sections = [
  { name: 'site-settings', labelKey: 'settings.general' },
  { name: 'site-settings-tracking', labelKey: 'settings.tracking.title' },
  { name: 'site-settings-sessions', labelKey: 'settings.sessions.title' },
  { name: 'site-settings-identity', labelKey: 'settings.identity.title' },
  { name: 'site-settings-retention', labelKey: 'dataRetention.title' },
  { name: 'recording-settings', labelKey: 'recording.title' },
  { name: 'site-settings-sharing', labelKey: 'sharing.title' },
] as const satisfies { name: string; labelKey: MessageKey }[]
</script>

<template>
  <SiteBoundary
    v-slot="{ site, revision }"
    :site-id="siteId"
  >
    <PageHeader
      :title="t('settings.title')"
      :description="site.domain"
    />
    <nav
      :aria-label="t('settings.sections')"
      :class="[segmentedTrack, 'mb-8']"
    >
      <RouterLink
        v-for="section in sections"
        :key="section.name"
        v-slot="{ href, navigate, isExactActive }"
        :to="{ name: section.name, params: { siteId } }"
        custom
      >
        <a
          :href="href"
          :aria-current="isExactActive ? 'page' : undefined"
          :data-state="isExactActive ? 'active' : undefined"
          :class="segmentedItem({ size: 'md' })"
          @click="navigate"
        >
          {{ t(section.labelKey) }}
        </a>
      </RouterLink>
    </nav>
    <RouterView v-slot="{ Component }">
      <component
        :is="Component"
        :key="site.id"
        class="max-w-5xl"
        :site="site"
        :revision="revision"
      />
    </RouterView>
  </SiteBoundary>
</template>
