<script setup lang="ts">
import { EmptyState } from '~/components/ui/empty-state'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

/** Shown only when the account belongs to no workspace; otherwise the router opens a site. */
const { t } = useI18n()
usePageMeta(() => t('home.title'))
</script>

<template>
  <EmptyState
    icon="lucide:building-2"
    :title="t('home.noWorkspaces')"
    :description="t('home.noWorkspacesHint')"
    class="mx-auto max-w-lg"
  />
</template>
