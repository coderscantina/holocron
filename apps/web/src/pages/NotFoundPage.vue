<script setup lang="ts">
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
usePageMeta(() => t('notFound.title'))
</script>

<template>
  <EmptyState
    icon="lucide:map-pin-off"
    :title="t('notFound.title')"
    :description="t('notFound.description')"
    class="mx-auto max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'home' }">{{ t('site.unavailable.home') }}</RouterLink>
    </Button>
  </EmptyState>
</template>
