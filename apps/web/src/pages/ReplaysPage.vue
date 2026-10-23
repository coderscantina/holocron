<script setup lang="ts">
import { refDebounced } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { Skeleton } from '~/components/ui/skeleton'
import { Switch } from '~/components/ui/switch'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { usePredicateText } from '~/composables/usePredicateText'
import { isReplayRange, lastDays, replayRanges, useReplays, type ReplayRange } from '~/composables/useRecordings'
import { useI18n } from '~/plugins/i18n'
import { canonical, decodeFilter } from '~/services/exploration'

import ReplayList from '~/components/recordings/ReplayList.vue'

import FilterMenu from './recordings/FilterMenu.vue'

const devices = ['all', 'desktop', 'mobile', 'tablet'] as const
type Device = (typeof devices)[number]

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { message } = useErrorMessage()
const { text: filterText } = usePredicateText()

usePageMeta(() => t('nav.replays'))

const siteId = computed(() => String(route.params.siteId))
const queryText = (name: string) => (typeof route.query[name] === 'string' ? route.query[name] : '')

/** Filters live in the URL, so a report can link straight to the replays it means. */
const setQuery = (patch: Record<string, string | undefined>) =>
  router.replace({ query: { ...route.query, ...patch } })

const range = computed<ReplayRange>({
  get: () => {
    const value = queryText('range')
    return isReplayRange(value) ? value : '7d'
  },
  set: (value) => setQuery({ range: value }),
})
const device = computed<Device>({
  get: () => devices.find((entry) => entry === queryText('device')) ?? 'all',
  set: (value) => setQuery({ device: value === 'all' ? undefined : value }),
})
const savedOnly = computed({
  get: () => queryText('saved') === '1',
  set: (value: boolean) => setQuery({ saved: value ? '1' : undefined }),
})
const path = ref(queryText('path'))
const debouncedPath = refDebounced(path, 300)
watch(debouncedPath, (value) => setQuery({ path: value || undefined }))
const session = computed(() => {
  const value = Number(queryText('session'))
  return Number.isInteger(value) && queryText('session') !== '' ? value : undefined
})

/**
 * `filter` holds a filter tree as `encodeFilter` writes it, for links from findings. An
 * unreadable one goes to the server as it is, so the list is refused instead of unfiltered.
 */
const filter = computed(() => (queryText('filter') ? decodeFilter(queryText('filter')) : undefined))
const filterArg = computed(() => {
  if (!filter.value) return undefined
  return filter.value.ok ? canonical(filter.value.filter) : queryText('filter')
})

const args = computed(() => ({
  site: siteId.value,
  ...lastDays(replayRanges[range.value]),
  path: queryText('path') || undefined,
  device: device.value === 'all' ? undefined : device.value,
  saved: savedOnly.value || undefined,
  session: session.value,
  filter: filterArg.value,
}))
const replays = useReplays(args)
const captures = computed(() => replays.data.value?.pages.flatMap((page) => page.captures) ?? [])

const rangeOptions = computed(() =>
  Object.keys(replayRanges)
    .filter(isReplayRange)
    .map((value) => ({ value, label: t(`replays.ranges.${value}`) }))
)
const deviceOptions = computed(() => devices.map((value) => ({ value, label: t(`replays.devices.${value}`) })))
</script>

<template>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <PageHeader
      :title="t('nav.replays')"
      :description="site.domain"
    >
      <template #actions>
        <div
          class="flex flex-wrap items-center gap-2"
          role="group"
          :aria-label="t('replays.listLabel')"
        >
          <FilterMenu
            v-model="range"
            :label="t('replays.range')"
            :options="rangeOptions"
          />
          <FilterMenu
            v-model="device"
            :label="t('replays.device')"
            :options="deviceOptions"
          />
          <Label
            for="replay-path"
            class="sr-only"
          >
            {{ t('replays.path') }}
          </Label>
          <Input
            id="replay-path"
            v-model="path"
            class="h-8 w-44"
            :placeholder="t('replays.path')"
            autocomplete="off"
          />
          <div class="flex items-center gap-2">
            <Switch
              id="replay-saved"
              v-model="savedOnly"
            />
            <Label for="replay-saved">{{ t('replays.savedOnly') }}</Label>
          </div>
        </div>
        <Button
          v-if="site.capabilities.includes('site.configure')"
          as-child
          variant="fill"
          class="text-sm"
        >
          <RouterLink :to="{ name: 'recording-settings', params: { siteId } }">
            <Icon
              name="lucide:settings-2"
              aria-hidden="true"
            />
            {{ t('recording.title') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <Alert
      v-if="session !== undefined"
      color="info"
      class="mb-4"
    >
      <p class="flex flex-wrap items-center gap-x-3 text-sm">
        {{ t('replays.session', { id: session }) }}
        <RouterLink
          class="font-semibold underline underline-offset-4"
          :to="{ query: { ...route.query, session: undefined } }"
        >
          {{ t('replays.clearSession') }}
        </RouterLink>
      </p>
    </Alert>

    <Alert
      v-if="filter?.ok"
      color="info"
      class="mb-4"
    >
      <p class="flex flex-wrap items-center gap-x-3 text-sm">
        {{ t('replays.filter', { filter: filterText(filter.filter) }) }}
        <RouterLink
          class="font-semibold underline underline-offset-4"
          :to="{ query: { ...route.query, filter: undefined } }"
        >
          {{ t('replays.clearFilter') }}
        </RouterLink>
      </p>
    </Alert>

    <EmptyState
      v-if="filter && !filter.ok"
      icon="lucide:triangle-alert"
      tone="error"
      :title="t('replays.filterUnreadable')"
    >
      <Button
        as-child
        variant="outline"
      >
        <RouterLink :to="{ query: { ...route.query, filter: undefined } }">{{ t('replays.clearFilter') }}</RouterLink>
      </Button>
    </EmptyState>
    <div
      v-else-if="replays.isPending.value"
      class="grid gap-2"
      :aria-label="t('states.loading')"
      aria-busy="true"
    >
      <Skeleton
        v-for="row in 5"
        :key="row"
        class="h-table-row w-full"
      />
    </div>
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
      :description="t('replays.empty.description')"
    >
      <Button
        v-if="site.capabilities.includes('site.configure')"
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'recording-settings', params: { siteId } }">{{ t('recording.open') }}</RouterLink>
      </Button>
    </EmptyState>
    <ReplayList
      v-else
      :captures="captures"
      :link="(capture) => ({ name: 'replay', params: { siteId, captureId: capture.id } })"
      :more="replays.hasNextPage.value"
      :loading-more="replays.isFetchingNextPage.value"
      @more="replays.fetchNextPage()"
    />
  </SiteBoundary>
</template>
