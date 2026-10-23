<script setup lang="ts">
import PageHeader from '~/components/PageHeader.vue'
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/** Installation administration. The router shows it to install admins only; the server checks every call. */
const { t } = useI18n()
usePageMeta(() => t('admin.title'))

const sections = [
  { name: 'admin-users', labelKey: 'admin.users.title' },
  { name: 'admin-security', labelKey: 'admin.security.title' },
  { name: 'admin-status', labelKey: 'admin.status.title' },
] as const satisfies { name: string; labelKey: MessageKey }[]
</script>

<template>
  <PageHeader :title="t('admin.title')" />
  <nav
    :aria-label="t('admin.sections')"
    :class="[segmentedTrack, 'mb-8']"
  >
    <RouterLink
      v-for="section in sections"
      :key="section.name"
      v-slot="{ href, navigate, isExactActive }"
      :to="{ name: section.name }"
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
  <RouterView />
</template>
