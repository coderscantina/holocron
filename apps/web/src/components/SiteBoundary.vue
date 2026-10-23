<script setup lang="ts">
import { ApiError } from '~/api'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useSite } from '~/composables/useSiteContext'
import { useI18n } from '~/plugins/i18n'

/**
 * Loads the route's site through `sites.get` and renders the slot only once the server has
 * authorized it. A guessed or hidden ID shows the same "not available" state as a missing one.
 */
const props = defineProps<{ siteId: string }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const query = useSite(() => props.siteId)

const unavailable = computed(() => query.error.value instanceof ApiError && query.error.value.code === 'not_found')
</script>

<template>
  <div
    v-if="query.isPending.value"
    class="grid gap-4"
    :aria-label="t('states.loading')"
    aria-busy="true"
  >
    <Skeleton class="h-10 w-64" />
    <Skeleton class="h-32 w-full" />
    <Skeleton class="h-64 w-full" />
  </div>
  <EmptyState
    v-else-if="unavailable"
    icon="lucide:shield-question"
    :title="t('site.unavailable.title')"
    :description="t('site.unavailable.description')"
    class="mx-auto max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'home' }">{{ t('site.unavailable.home') }}</RouterLink>
    </Button>
  </EmptyState>
  <EmptyState
    v-else-if="query.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="t('site.loadFailed')"
    :description="message(query.error.value)"
    class="mx-auto max-w-lg"
  >
    <Button
      variant="outline"
      @click="query.refetch()"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <slot
    v-else-if="query.data.value"
    :site="query.data.value.site"
    :revision="query.data.value.revision"
  />
</template>
