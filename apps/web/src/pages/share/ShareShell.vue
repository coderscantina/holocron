<script setup lang="ts">
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useShare } from '~/composables/useSharedView'
import { useI18n } from '~/plugins/i18n'

/**
 * Resolves one share and renders its page once it is known. A share that is gone (revoked,
 * expired, never existed) shows one "not available" state for every page under it.
 */
const props = defineProps<{ token: string }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const share = useShare(props.token)

usePageMeta(() => share.view.data.value?.name ?? t('share.title'))
</script>

<template>
  <EmptyState
    v-if="share.gone.value"
    icon="lucide:link-2-off"
    :title="t('share.unavailable')"
    :description="t('share.unavailableHint')"
    class="mx-auto mt-12 max-w-lg"
  />
  <div
    v-else-if="share.view.isPending.value"
    class="grid gap-4"
    aria-busy="true"
    :aria-label="t('states.loading')"
  >
    <Skeleton class="h-10 w-64" />
    <Skeleton class="h-32 w-full" />
    <Skeleton class="h-64 w-full" />
  </div>
  <EmptyState
    v-else-if="share.view.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="t('share.loadFailed')"
    :description="message(share.view.error.value)"
    class="mx-auto mt-12 max-w-lg"
  >
    <Button
      variant="outline"
      @click="share.view.refetch()"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <RouterView
    v-else-if="share.view.data.value"
    v-slot="{ Component }"
  >
    <component
      :is="Component"
      :share="share"
      :view="share.view.data.value"
    />
  </RouterView>
</template>
