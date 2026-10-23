<script setup lang="ts">
import CountryLabel from '~/components/CountryLabel.vue'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { explorationsResource } from '~/api/resources/explorations'
import { Alert } from '~/components/ui/alert'
import { Card } from '~/components/ui/card'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import { useLiveSnapshot, type LiveStatus } from '~/composables/useLive'
import { useViewMode } from '~/composables/useViewMode'
import type { RecentKind, Site } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import RealtimeGlobeView from './RealtimeGlobeView.vue'

/**
 * The last 30 minutes as they arrive: the snapshot is read once, then again at most once per
 * batch the live subscription reports. The connection state is visible, so stale figures
 * never pass for current ones. Two views of the same snapshot: the globe (default) and the
 * details below; the choice is remembered in this browser.
 */
const props = defineProps<{ site: Site }>()

const { t, locale } = useI18n()
const { message } = useErrorMessage()
const format = useFormat()
const live = useLiveSnapshot(
  () => props.site.id,
  (site, signal) => explorationsResource.realtime(site, signal)
)
const data = live.data

const view = useViewMode('realtime', ['globe', 'details'])
const views = computed(() => [
  { value: 'globe' as const, label: t('realtime.view.globe') },
  { value: 'details' as const, label: t('realtime.view.details') },
])

const statusLabels: Record<LiveStatus, MessageKey> = {
  connecting: 'realtime.status.connecting',
  current: 'realtime.status.current',
  stale: 'realtime.status.stale',
  resyncing: 'realtime.status.resyncing',
  disconnected: 'realtime.status.disconnected',
}
const kindLabels: Record<RecentKind, MessageKey> = {
  pageview: 'realtime.kind.pageview',
  custom_event: 'realtime.kind.custom_event',
}

const max = computed(() => Math.max(1, ...(data.value?.series ?? []).map((b) => b.pageviews)))
const totals = computed(() => ({
  views: (data.value?.series ?? []).reduce((sum, b) => sum + b.pageviews, 0),
  visitors: (data.value?.series ?? []).reduce((sum, b) => sum + b.visitors, 0),
}))
const time = (ms: number) =>
  new Intl.DateTimeFormat(locale.value, { timeZone: props.site.timezone, timeStyle: 'medium' }).format(ms)
const minute = (ms: number) =>
  new Intl.DateTimeFormat(locale.value, { timeZone: props.site.timezone, hour: '2-digit', minute: '2-digit' }).format(ms)
const recentId = useId()
</script>

<template>
  <PageHeader
    :title="t('realtime.title')"
    :description="site.domain"
  >
    <template #actions>
      <ChoiceGroup
        v-model="view"
        :label="t('realtime.view.label')"
        :options="views"
      />
      <span
        class="inline-flex items-center gap-1.5 rounded-md border border-hairline px-2 py-1 text-xs font-semibold"
        :class="live.status.value === 'current' ? 'text-success' : 'text-muted'"
        role="status"
        aria-live="polite"
      >
        <span
          class="size-2 rounded-full"
          :class="live.status.value === 'current' ? 'bg-success motion-safe:animate-pulse' : 'bg-series-comparison'"
          aria-hidden="true"
        />
        {{ t(statusLabels[live.status.value]) }}
      </span>
    </template>
  </PageHeader>

  <Alert
    v-if="live.error.value && !data"
    color="destructive"
    role="alert"
    class="mb-4"
  >
    <p class="text-sm">{{ message(live.error.value) }}</p>
  </Alert>

  <RealtimeGlobeView
    v-if="data && view !== 'details'"
    :data="data"
  />
  <template v-else-if="data">
    <div class="mb-4 grid gap-4 lg:grid-cols-[16rem_1fr]">
      <Card>
        <p class="flex items-center gap-1 text-sm font-medium text-muted">
          {{ t('realtime.active') }}
          <SimpleTooltip
            :tooltip="t('realtime.activeHint')"
            :aria-label="t('realtime.activeHint')"
            class="-my-1 inline-flex size-6 cursor-help items-center justify-center rounded-control hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          >
            <Icon
              name="lucide:info"
              size="14"
              aria-hidden="true"
            />
          </SimpleTooltip>
        </p>
        <p class="mt-2 text-[2.75rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.integer(data.active) }}</p>
      </Card>
      <Panel :title="t('realtime.minutes')">
        <figure class="m-0">
          <figcaption class="sr-only">
            {{ t('realtime.summary', { views: totals.views, visitors: totals.visitors }) }}
          </figcaption>
          <div
            class="flex h-32 items-end gap-[2px]"
            aria-hidden="true"
          >
            <div
              v-for="bucket in data.series"
              :key="bucket.start"
              class="relative h-full flex-1"
            >
              <span
                class="absolute inset-x-0 bottom-0 rounded-t-[2px] bg-series-primary"
                :style="{ height: `${(bucket.pageviews / max) * 100}%` }"
              />
            </div>
          </div>
          <div
            class="mt-2 flex justify-between text-[0.8125rem] text-muted tabular-nums"
            aria-hidden="true"
          >
            <span>{{ minute(data.series[0]?.start ?? data.now) }}</span>
            <span>{{ minute(data.now) }}</span>
          </div>
          <div class="sr-only">
            <table>
              <caption>{{ t('realtime.minutes') }}</caption>
              <thead>
                <tr>
                  <th scope="col">{{ t('realtime.minute') }}</th>
                  <th scope="col">{{ t('explore.measures.pageviews') }}</th>
                  <th scope="col">{{ t('explore.measures.visitors') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="bucket in data.series"
                  :key="bucket.start"
                >
                  <th scope="row">{{ minute(bucket.start) }}</th>
                  <td>{{ bucket.pageviews }}</td>
                  <td>{{ bucket.visitors }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </figure>
      </Panel>
    </div>

    <Panel
      :id="recentId"
      :title="t('realtime.recent')"
      flush
    >
      <div
        role="region"
        tabindex="0"
        :aria-labelledby="recentId"
        class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
      >
        <table
          class="w-full text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider"
          :aria-labelledby="recentId"
        >
          <thead class="bg-fill text-[0.875rem] text-muted">
            <tr>
              <th
                scope="col"
                class="h-table-head px-4 text-left font-medium"
              >
                {{ t('realtime.columns.time') }}
              </th>
              <th
                scope="col"
                class="h-table-head px-4 text-left font-medium"
              >
                {{ t('realtime.columns.what') }}
              </th>
              <th
                scope="col"
                class="h-table-head px-4 text-left font-medium"
              >
                {{ t('realtime.columns.country') }}
              </th>
              <th
                scope="col"
                class="h-table-head px-4 text-left font-medium max-sm:hidden"
              >
                {{ t('realtime.columns.device') }}
              </th>
              <th
                scope="col"
                class="h-table-head px-4 text-left font-medium max-sm:hidden"
              >
                {{ t('realtime.columns.browser') }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="(event, index) in data.recent"
              :key="index"
              class="h-table-row border-t border-divider"
            >
              <td class="px-4 whitespace-nowrap text-muted tabular-nums">{{ time(event.at) }}</td>
              <td class="max-w-80 truncate px-4 text-primary">
                <span class="mr-1.5 text-xs text-muted">{{ t(kindLabels[event.kind]) }}</span>
                {{ event.kind === 'pageview' ? event.path : event.event_name }}
              </td>
              <td class="max-w-48 px-4">
                <CountryLabel
                  v-if="event.country"
                  :code="event.country"
                  class="max-w-full"
                />
                <template v-else>–</template>
              </td>
              <td class="px-4 max-sm:hidden">{{ event.device ?? '–' }}</td>
              <td class="px-4 max-sm:hidden">{{ event.browser ?? '–' }}</td>
            </tr>
            <tr
              v-if="data.recent.length === 0"
              class="border-t border-divider"
            >
              <td
                colspan="5"
                class="px-4 py-6 text-center text-muted"
              >
                {{ t('realtime.noRecent') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </Panel>
  </template>
  <div
    v-else-if="!live.error.value"
    class="grid gap-4"
  >
    <Skeleton class="h-32 w-full" />
    <Skeleton class="h-64 w-full" />
  </div>
</template>
