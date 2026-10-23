<script setup lang="ts">
// Moved out of ReplayPage.vue, so share viewers play replays with the same player and frame.
import type { RouteLocationRaw } from 'vue-router'

import { ApiError } from '~/api'
import type { ShareScope } from '~/api/resources/sharing'
import CountryLabel from '~/components/CountryLabel.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import { useReplay } from '~/composables/useRecordings'
import ReplayPlayer from '~/pages/recordings/ReplayPlayer.vue'
import ReplayTimeline from '~/pages/recordings/ReplayTimeline.vue'
import { useI18n } from '~/plugins/i18n'

/**
 * One replay: the player in the isolated replay frame, its details and timeline. A replay the
 * caller cannot reach shows the not-found state, the same as a missing one.
 */
const props = defineProps<{
  siteId: string
  captureId: string
  /** Reads as a share viewer. */
  share?: ShareScope
  /** Where the replay's session leads; share viewers have no session list. */
  sessionLink?: (session: number) => RouteLocationRaw
}>()

const { t } = useI18n()
const format = useFormat()
const { message } = useErrorMessage()

const replay = useReplay(
  () => props.siteId,
  () => props.captureId,
  props.share
)
const player = ref<InstanceType<typeof ReplayPlayer> | null>(null)
const start = ref(0)
const current = ref(0)

// The player asks for the rest of the replay page by page while the first one plays.
watch(
  () => [replay.hasNextPage.value, replay.isFetchingNextPage.value] as const,
  ([more, fetching]) => {
    if (more && !fetching) replay.fetchNextPage()
  }
)

const pages = computed(() => replay.data.value?.pages ?? [])
const capture = computed(() => pages.value[0]?.capture ?? null)
const events = computed(() => pages.value.map((page) => page.events))
const timeline = computed(() => pages.value.flatMap((page) => page.timeline))
const gaps = computed(() => pages.value.reduce((sum, page) => sum + page.gaps, 0))
const missing = computed(() => replay.error.value instanceof ApiError && replay.error.value.code === 'not_found')
</script>

<template>
  <div
    v-if="replay.isPending.value"
    class="grid gap-4"
    :aria-label="t('replays.player.loading')"
    aria-busy="true"
  >
    <Skeleton class="aspect-video w-full" />
  </div>
  <EmptyState
    v-else-if="missing"
    icon="lucide:shield-question"
    :title="t('replays.unavailable')"
    :description="t('errors.not_found')"
  />
  <EmptyState
    v-else-if="replay.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="message(replay.error.value)"
  >
    <Button
      variant="outline"
      @click="replay.refetch()"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <div
    v-else-if="capture"
    class="grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]"
  >
    <div class="grid min-w-0 gap-3">
      <Alert
        v-if="capture.truncated"
        color="warning"
      >
        <p class="text-sm">{{ t('replays.player.truncated') }}</p>
      </Alert>
      <Alert
        v-if="gaps > 0"
        color="warning"
      >
        <p class="text-sm">{{ t('replays.player.gaps', { count: gaps }) }}</p>
      </Alert>
      <Panel
        flush
        class="p-3"
      >
        <ReplayPlayer
          ref="player"
          :pages="events"
          :loading="replay.hasNextPage.value"
          @start="start = $event"
          @time="current = $event"
        />
      </Panel>
    </div>

    <aside class="grid min-w-0 gap-4">
      <Panel :title="t('replays.player.details')">
        <div class="grid gap-3">
          <dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm">
            <dt class="text-muted">{{ t('replays.columns.duration') }}</dt>
            <dd class="text-primary tabular-nums">{{ format.duration(capture.duration_ms / 1000) }}</dd>
            <dt class="text-muted">{{ t('replays.columns.entry') }}</dt>
            <dd
              class="truncate text-primary"
              :title="capture.entry_path"
            >
              {{ capture.entry_path ?? '' }}
            </dd>
            <dt class="text-muted">{{ t('replays.columns.pages') }}</dt>
            <dd class="text-primary tabular-nums">{{ format.integer(capture.pages) }}</dd>
            <dt class="text-muted">{{ t('replays.columns.device') }}</dt>
            <dd class="truncate text-primary">
              {{ [capture.device, capture.browser, capture.os].filter(Boolean).join(' · ') }}
            </dd>
            <template v-if="capture.country">
              <dt class="text-muted">{{ t('replays.columns.country') }}</dt>
              <dd class="min-w-0 text-primary">
                <CountryLabel
                  :code="capture.country"
                  class="max-w-full"
                />
              </dd>
            </template>
          </dl>
          <p
            v-if="capture.saved"
            class="text-xs text-muted"
          >
            {{ t('replays.player.savedHint') }}
          </p>
          <RouterLink
            v-if="capture.session !== undefined && sessionLink"
            :to="sessionLink(capture.session)"
            class="inline-flex items-center gap-1.5 text-sm font-medium text-accent-text hover:underline"
          >
            <Icon
              name="lucide:users"
              aria-hidden="true"
            />
            {{ t('replays.player.sessionLink') }}
          </RouterLink>
        </div>
      </Panel>
      <Panel :title="t('replays.timeline.title')">
        <div class="-mx-2 max-h-[28rem] overflow-y-auto">
          <ReplayTimeline
            :entries="timeline"
            :start="start"
            :current="current"
            @seek="player?.seekTo($event)"
          />
        </div>
      </Panel>
    </aside>
  </div>
</template>
