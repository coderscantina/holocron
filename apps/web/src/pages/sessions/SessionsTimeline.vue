<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import { recordingsResource } from '~/api/resources/recordings'
import CountryFlag from '~/components/CountryFlag.vue'
import PageHeader from '~/components/PageHeader.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import StageView from '~/components/stage/StageView.vue'
import { Alert } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { watchScope } from '~/composables/useExplore'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useViewMode } from '~/composables/useViewMode'
import type { Site } from '~/generated'
import { entryOf, longest } from '~/lib/session-timeline'
import { useI18n } from '~/plugins/i18n'

import SessionLanes from './SessionLanes.vue'

/**
 * The sessions view's default: the latest sessions of the last hours as lines over time, beside
 * their count, the median observed span and the longest of them. Read again every minute while
 * the page is visible. Replays come from the recordings list of the same window, for people who
 * may read recordings.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useFormat()
const explorationFormat = useExplorationFormat()
const { message } = useErrorMessage()

const MINUTE_MS = 60_000
const LONGEST = 5

const windows = { '1h': 1, '6h': 6, '24h': 24 } as const
const period = useViewMode('sessions-window', ['6h', '1h', '24h'])
const hours = computed(() => windows[period.value])
const windowOptions = computed(() => [
  { value: '1h' as const, label: t('reports.sessions.timeline.windows.1h') },
  { value: '6h' as const, label: t('reports.sessions.timeline.windows.6h') },
  { value: '24h' as const, label: t('reports.sessions.timeline.windows.24h') },
])

const query = useQuery({
  queryKey: computed(() => queryKeys.sessionTimeline(props.site.id, hours.value)),
  queryFn: ({ signal }) => explorationsResource.sessionTimeline(props.site.id, hours.value, signal),
  refetchInterval: MINUTE_MS,
  placeholderData: (previous) => previous,
})
watchScope(() => props.site.id, () => query.error.value)
const timeline = computed(() => query.data.value ?? null)

const canReplay = computed(() => props.site.capabilities.includes('site.recordings.read'))
const replaysQuery = useQuery({
  queryKey: computed(() => queryKeys.timelineReplays(props.site.id, hours.value)),
  queryFn: ({ signal }) => {
    const current = timeline.value
    return recordingsResource.list({ site: props.site.id, start: current?.from ?? 0, end: current?.to ?? 0, limit: 100 }, signal)
  },
  enabled: computed(() => canReplay.value && timeline.value !== null),
  staleTime: MINUTE_MS,
})
const replays = computed(() => {
  const map = new Map<number, string>()
  for (const capture of replaysQuery.data.value?.captures ?? []) {
    if (capture.session !== undefined && !map.has(capture.session)) map.set(capture.session, capture.id)
  }
  return map
})

const capped = computed(() => (timeline.value ? timeline.value.total > timeline.value.sessions.length : false))
const withEvents = computed(() => timeline.value?.sessions.filter((session) => session.custom_events > 0).length ?? 0)
const longestSessions = computed(() => longest(timeline.value?.sessions ?? [], LONGEST))
const duration = (ms: number) => format.duration(Math.round(ms / 1000))
</script>

<template>
  <PageHeader
    :title="t('reports.sessions.title')"
    :description="site.domain"
  >
    <template #eyebrow>
      <slot name="context" />
    </template>
    <template #actions>
      <ChoiceGroup
        v-model="period"
        :label="t('reports.sessions.timeline.window')"
        :options="windowOptions"
      />
    </template>
  </PageHeader>

  <Alert
    v-if="query.isError.value && !timeline"
    color="destructive"
    role="alert"
    class="mb-4"
  >
    <p class="text-sm">{{ message(query.error.value) }}</p>
  </Alert>

  <StageView :label="t('reports.sessions.timeline.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <template v-if="timeline">
          <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.integer(timeline.total) }}</p>
          <p class="mt-3 text-[0.9375rem] text-muted">{{ t(`reports.sessions.timeline.started.${period}`) }}</p>
          <dl class="mt-6 flex flex-col gap-2 text-[0.9375rem]">
            <div class="flex items-baseline justify-between gap-3">
              <dt class="text-muted">{{ t('reports.sessions.timeline.median') }}</dt>
              <dd class="font-semibold text-primary tabular-nums">
                {{ timeline.median_span === undefined ? '–' : duration(timeline.median_span) }}
              </dd>
            </div>
            <div class="flex items-baseline justify-between gap-3">
              <dt class="flex items-center gap-2 text-muted">
                <span
                  class="size-2 rounded-full bg-accent"
                  aria-hidden="true"
                />
                {{ t('reports.sessions.timeline.withEvents') }}
              </dt>
              <dd class="font-semibold text-primary tabular-nums">{{ format.integer(withEvents) }}</dd>
            </div>
          </dl>
          <p
            v-if="capped"
            class="mt-4 text-sm text-muted"
          >
            {{ t('reports.sessions.timeline.capped', { shown: format.integer(timeline.sessions.length), total: format.integer(timeline.total) }) }}
          </p>
        </template>
        <template v-else>
          <Skeleton class="h-13 w-32" />
          <Skeleton class="mt-3 h-5 w-48" />
          <Skeleton class="mt-6 h-12 w-full" />
        </template>
      </div>

      <h2 class="pt-5 pb-2 text-sm font-medium text-muted">{{ t('reports.sessions.timeline.longest') }}</h2>
      <ul
        v-if="longestSessions.length > 0"
        class="-mx-2 flex flex-col"
      >
        <li
          v-for="session in longestSessions"
          :key="session.session"
        >
          <RouterLink
            :to="{ name: 'session', params: { siteId: site.id, sessionId: session.session } }"
            class="flex h-10 items-center gap-3 rounded-control px-2 text-[0.9375rem] hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
          >
            <CountryFlag
              v-if="session.country"
              :code="session.country"
            />
            <span
              v-else
              class="w-4.5 shrink-0"
              aria-hidden="true"
            />
            <span
              v-if="session.country"
              class="sr-only"
            >
              {{ explorationFormat.textLabel('country', session.country) }}
            </span>
            <span class="min-w-0 flex-1 truncate text-primary">{{ entryOf(session) ?? '–' }}</span>
            <span
              v-if="session.custom_events > 0"
              class="size-2 shrink-0 rounded-full bg-accent"
              :title="t('reports.sessions.timeline.customEvents', { count: session.custom_events })"
            />
            <span class="shrink-0 text-muted tabular-nums">{{ duration(session.end - session.start) }}</span>
          </RouterLink>
        </li>
      </ul>
      <p
        v-else-if="timeline"
        class="text-[0.9375rem] text-muted"
      >
        {{ t('reports.sessions.timeline.noLongest') }}
      </p>
    </template>

    <SessionLanes
      v-if="timeline && timeline.sessions.length > 0"
      :site="site"
      :timeline="timeline"
      :replays="replays"
    />
    <p
      v-else-if="timeline"
      class="flex size-full min-h-[22rem] items-center justify-center text-muted"
    >
      {{ t(`reports.sessions.timeline.empty.${period}`) }}
    </p>
    <Skeleton
      v-else
      class="size-full min-h-[22rem]"
    />
  </StageView>
</template>
