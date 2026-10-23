<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ErrorBoundary.vue.
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const route = useRoute()

const error = ref<Error | null>(null)

/**
 * Catches render, watcher and lifecycle failures in the page, so the shell stays usable and
 * navigating away is a way out. Returning false stops the error from travelling further up.
 */
onErrorCaptured((caught) => {
  error.value = caught instanceof Error ? caught : new Error(String(caught))
  console.error('[error-boundary]', caught)
  return false
})

// Navigating away is a retry: the failing subtree is gone either way.
watch(
  () => route.fullPath,
  () => (error.value = null)
)
</script>

<template>
  <EmptyState
    v-if="error"
    icon="lucide:triangle-alert"
    tone="error"
    :title="t('errorBoundary.title')"
    :description="t('errorBoundary.description')"
    class="mx-auto max-w-lg"
  >
    <Button
      variant="outline"
      @click="error = null"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <slot v-else />
</template>
