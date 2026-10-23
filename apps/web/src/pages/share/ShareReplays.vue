<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ReplayList from '~/components/recordings/ReplayList.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { isRefusal } from '~/composables/useExplore'
import { lastDays, replayRanges, useReplays } from '~/composables/useRecordings'
import type { Share } from '~/composables/useSharedView'
import type { SharedView } from '~/generated'
import FilterMenu from '~/pages/recordings/FilterMenu.vue'
import { useI18n } from '~/plugins/i18n'

import ShareHeader from './ShareHeader.vue'

/**
 * Replays a share grants: its sites, inside its range limit and pinned filter, which the server
 * applies. Without the grant the server refuses the list, and the page says so.
 */
const props = defineProps<{ share: Share; view: SharedView }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()

const token = computed(() => String(route.params.token))
const site = computed(() => {
  const wanted = typeof route.query.site === 'string' ? route.query.site : null
  const target = props.view.target
  const fallback = target.kind === 'exploration' ? target.state.site : props.view.sites[0]?.id
  return props.view.sites.find((entry) => entry.id === wanted)?.id ?? fallback ?? ''
})
/** Replays are kept 14 days; an exploration share may allow less. */
const days = computed(() => {
  const target = props.view.target
  return target.kind === 'exploration' ? Math.min(replayRanges['14d'], target.limits.range_max_days) : replayRanges['14d']
})

const replays = useReplays(() => ({ site: site.value, ...lastDays(days.value) }), props.share.scope)
const captures = computed(() => replays.data.value?.pages.flatMap((page) => page.captures) ?? [])
// A refusal can also mean the share itself ended: resolving it again tells which.
watch(
  () => replays.error.value,
  (error) => {
    if (isRefusal(error)) void props.share.view.refetch()
  }
)

const siteOptions = computed(() => props.view.sites.map((entry) => ({ value: entry.id, label: entry.name })))
const pickSite = computed({
  get: () => site.value,
  set: (value: string) => router.replace({ query: { ...route.query, site: value } }),
})
</script>

<template>
  <ShareHeader
    :view="view"
    :title="t('nav.replays')"
  >
    <Button
      as-child
      variant="outline"
      size="sm"
    >
      <RouterLink :to="{ name: 'share', params: { token } }">
        <Icon
          name="lucide:arrow-left"
          aria-hidden="true"
        />
        {{ t('share.back') }}
      </RouterLink>
    </Button>
  </ShareHeader>
  <div
    v-if="siteOptions.length > 1"
    class="mb-4"
  >
    <FilterMenu
      v-model="pickSite"
      :label="t('share.site')"
      :options="siteOptions"
    />
  </div>
  <div
    v-if="replays.isPending.value"
    class="grid gap-2"
    :aria-label="t('states.loading')"
    aria-busy="true"
  >
    <Skeleton
      v-for="row in 5"
      :key="row"
      class="h-12 w-full"
    />
  </div>
  <EmptyState
    v-else-if="isRefusal(replays.error.value)"
    icon="lucide:shield-question"
    :title="t('share.noRecordings')"
    class="mx-auto max-w-lg"
  />
  <EmptyState
    v-else-if="replays.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="message(replays.error.value)"
  >
    <Button
      variant="outline"
      @click="replays.refetch()"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <EmptyState
    v-else-if="captures.length === 0"
    icon="lucide:circle-play"
    :title="t('replays.empty.title')"
    :description="t('share.noReplaysHint', { days })"
  />
  <ReplayList
    v-else
    :captures="captures"
    :link="(capture) => ({ name: 'share-replay', params: { token, captureId: capture.id }, query: { site } })"
    :more="replays.hasNextPage.value"
    :loading-more="replays.isFetchingNextPage.value"
    @more="replays.fetchNextPage()"
  />
</template>
