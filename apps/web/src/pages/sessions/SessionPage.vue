<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import CountryFlag from '~/components/CountryFlag.vue'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { usePageMeta } from '~/composables/usePageMeta'
import type { Field, SessionDetail, SessionEvent } from '~/generated'
import { valueIcon } from '~/lib/client-looks'
import { useI18n } from '~/plugins/i18n'

/**
 * One session: what it adds up to, what happened in order, and who it was. The timeline is the
 * page's subject; the visitor's fields sit beside it, and properties only when there are some.
 */
const route = useRoute()
const { t, locale } = useI18n()
const { message } = useErrorMessage()
const format = useExplorationFormat()
const numbers = useFormat()

const siteId = computed(() => String(route.params.siteId))
/** Back to the list as it was left (range, filters, page), or to a fresh list. */
const back = computed(() => {
  const previous: unknown = window.history.state?.back
  return typeof previous === 'string' && previous.startsWith(`/sites/${siteId.value}/sessions`)
    ? previous
    : { name: 'sessions', params: { siteId: siteId.value } }
})
const sessionId = computed(() => Number(route.params.sessionId))
usePageMeta(() => t('reports.sessions.detail.title'))

const query = useQuery({
  queryKey: computed(() => queryKeys.sessionDetail(siteId.value, sessionId.value)),
  queryFn: ({ signal }) => explorationsResource.session(siteId.value, sessionId.value, signal),
  enabled: computed(() => Number.isSafeInteger(sessionId.value)),
})
const missing = computed(
  () => !Number.isSafeInteger(sessionId.value) || (query.error.value instanceof ApiError && query.error.value.code === 'not_found')
)

const clock = (ms: number, timezone: string) =>
  new Intl.DateTimeFormat(locale.value, { timeZone: timezone, timeStyle: 'medium' }).format(ms)

const pages = (detail: SessionDetail) =>
  new Set(detail.events.filter((event) => event.kind === 'pageview').map((event) => event.path)).size

const stats = (detail: SessionDetail) => [
  { label: t('reports.sessions.columns.span'), value: numbers.duration((detail.end - detail.start) / 1000) },
  { label: t('reports.sessions.columns.pageviews'), value: numbers.integer(detail.pageviews) },
  { label: t('reports.sessions.columns.custom_events'), value: numbers.integer(detail.custom_events) },
  { label: t('reports.sessions.detail.pages'), value: numbers.integer(pages(detail)) },
]

/** Time since the previous event, shown only where the visitor actually paused. */
const pause = (events: SessionEvent[], index: number): string | null => {
  const previous = events[index - 1]
  const current = events[index]
  if (!previous || !current) return null
  const seconds = (current.at - previous.at) / 1000
  return seconds >= 1 ? t('reports.sessions.detail.after', { time: numbers.duration(seconds) }) : null
}

const joined = (...parts: (string | null | undefined)[]) => parts.filter(Boolean).join(' · ')

interface Fact {
  label: string
  value: string
  detail?: string
  icons?: string[]
  /** ISO country code whose flag leads the value. */
  flag?: string
}

const facts = (detail: SessionDetail): Fact[] => {
  const f = detail.fields
  const named = (field: Field, value?: string) => (value ? format.textLabel(field, value) : undefined)
  const icon = (field: Field, value?: string) => (value ? valueIcon(field, value) : null)
  const list: (Fact | null)[] = [
    f.channel || f.referrer_domain
      ? {
          label: t('reports.sessions.detail.source'),
          value: named('channel', f.channel) ?? f.referrer_domain ?? '',
          detail: f.referrer_domain ? `${f.referrer_domain}${f.referrer_path ?? ''}` : undefined,
        }
      : null,
    f.utm_source || f.utm_campaign
      ? {
          label: t('reports.sessions.detail.campaign'),
          value: f.utm_campaign ?? f.utm_source ?? '',
          detail: joined(f.utm_campaign ? f.utm_source : null, f.utm_medium) || undefined,
        }
      : null,
    f.country || f.city
      ? {
          label: t('reports.sessions.detail.location'),
          value: named('country', f.country) ?? f.city ?? '',
          detail: joined(f.country ? f.city : null, f.region) || undefined,
          flag: f.country ?? undefined,
        }
      : null,
    f.device || f.browser || f.os
      ? {
          label: t('reports.sessions.detail.device'),
          value: joined(named('browser', f.browser), named('os', f.os)),
          detail: named('device', f.device),
          icons: [icon('device', f.device), icon('os', f.os), icon('browser', f.browser)].filter(
            (name): name is string => name !== null
          ),
        }
      : null,
    f.language ? { label: t('reports.sessions.detail.language'), value: f.language } : null,
    f.screen ? { label: t('reports.sessions.detail.screen'), value: f.screen } : null,
    {
      label: t('reports.sessions.detail.identity'),
      value: t(detail.person ? 'reports.sessions.person' : 'reports.sessions.daily'),
    },
  ]
  return list.filter((fact): fact is Fact => fact !== null)
}

const copyId = async (id: number) => {
  try {
    await navigator.clipboard.writeText(String(id))
    toast.success(t('reports.sessions.detail.copied'))
  } catch (error) {
    toast.error(message(error))
  }
}
</script>

<template>
  <SiteBoundary :site-id="siteId">
    <template #default="{ site }">
      <PageHeader
        :title="t('reports.sessions.detail.title')"
        :description="query.data.value ? format.date(query.data.value.start, site.timezone, true) : undefined"
      >
        <template #eyebrow>
          <RouterLink
            :to="back"
            class="inline-flex items-center gap-1 hover:text-primary hover:underline"
          >
            <Icon
              name="lucide:arrow-left"
              size="12"
              aria-hidden="true"
            />
            {{ t('reports.sessions.detail.back') }}
          </RouterLink>
        </template>
      </PageHeader>

      <EmptyState
        v-if="missing"
        icon="lucide:search-x"
        :title="t('reports.sessions.detail.notFoundTitle')"
        :description="t('reports.sessions.detail.notFoundDescription')"
        class="mx-auto max-w-lg"
      />
      <EmptyState
        v-else-if="query.isError.value"
        icon="lucide:triangle-alert"
        tone="error"
        :title="message(query.error.value)"
        class="mx-auto max-w-lg"
      >
        <Button
          variant="fill"
          @click="query.refetch()"
        >
          {{ t('actions.retry') }}
        </Button>
      </EmptyState>
      <div
        v-else-if="!query.data.value"
        class="grid gap-4"
        aria-busy="true"
        :aria-label="t('states.loading')"
      >
        <Skeleton class="h-24 w-full rounded-panel sm:h-26" />
        <Skeleton class="h-96 w-full rounded-panel" />
      </div>
      <div
        v-else
        class="grid gap-4"
      >
        <dl class="grid grid-cols-2 gap-1 rounded-panel bg-fill p-1 sm:grid-cols-4">
          <div
            v-for="stat in stats(query.data.value)"
            :key="stat.label"
            class="min-w-0 rounded-control px-4 pt-3.5 pb-4 sm:px-5"
          >
            <dt class="truncate text-sm text-muted">{{ stat.label }}</dt>
            <dd class="mt-2 truncate text-[2rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums sm:text-[2.375rem]">
              {{ stat.value }}
            </dd>
          </div>
        </dl>

        <div class="grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
          <Panel
            id="session-activity"
            :title="t('reports.sessions.detail.activity')"
            :meta="
              t('reports.sessions.detail.span', {
                start: clock(query.data.value.start, site.timezone),
                end: clock(query.data.value.end, site.timezone),
              })
            "
            flush
          >
            <ol aria-labelledby="session-activity">
              <li
                v-for="(event, index) in query.data.value.events"
                :key="index"
                class="grid grid-cols-[4.75rem_1.25rem_minmax(0,1fr)_auto] items-center gap-x-3 border-t border-divider py-2.5 pr-4.5 pl-4.5"
              >
                <time
                  :datetime="new Date(event.at).toISOString()"
                  class="text-[0.875rem] text-muted tabular-nums"
                >
                  {{ clock(event.at, site.timezone) }}
                </time>
                <Icon
                  :name="event.kind === 'pageview' ? 'lucide:file-text' : 'lucide:zap'"
                  size="16"
                  :class="event.kind === 'pageview' ? 'text-muted' : 'text-accent-text'"
                  :aria-label="t(event.kind === 'pageview' ? 'reports.sessions.detail.pageview' : 'reports.sessions.detail.event')"
                />
                <span class="min-w-0">
                  <span class="block truncate text-sm text-primary">
                    {{ event.kind === 'pageview' ? (event.path ?? t('reports.sessions.none')) : event.event_name }}
                  </span>
                  <span
                    v-if="event.kind === 'pageview' ? event.title : event.path"
                    class="block truncate text-[0.875rem] text-muted"
                  >
                    {{ event.kind === 'pageview' ? event.title : event.path }}
                  </span>
                </span>
                <span class="text-[0.875rem] whitespace-nowrap text-muted tabular-nums">
                  {{ pause(query.data.value.events, index) }}
                </span>
              </li>
            </ol>
            <p
              v-if="query.data.value.truncated"
              class="border-t border-divider px-4.5 py-3 text-sm text-muted"
            >
              {{ t('reports.sessions.detail.truncated', { count: query.data.value.events.length }) }}
            </p>
          </Panel>

          <div class="grid gap-4">
            <Panel :title="t('reports.sessions.detail.visitor')">
              <dl class="grid gap-4">
                <div
                  v-for="fact in facts(query.data.value)"
                  :key="fact.label"
                  class="grid gap-0.5"
                >
                  <dt class="text-[0.875rem] text-muted">{{ fact.label }}</dt>
                  <dd class="flex min-w-0 items-center gap-2 text-sm text-primary">
                    <span
                      v-if="fact.icons?.length"
                      class="flex shrink-0 items-center gap-1 text-muted"
                      aria-hidden="true"
                    >
                      <Icon
                        v-for="name in fact.icons"
                        :key="name"
                        :name="name"
                        size="15"
                      />
                    </span>
                    <CountryFlag
                      v-if="fact.flag"
                      :code="fact.flag"
                    />
                    <span class="truncate">{{ fact.value }}</span>
                  </dd>
                  <dd
                    v-if="fact.detail"
                    class="truncate text-[0.875rem] text-muted"
                  >
                    {{ fact.detail }}
                  </dd>
                </div>
                <div class="grid gap-0.5">
                  <dt class="text-[0.875rem] text-muted">{{ t('reports.sessions.detail.sessionId') }}</dt>
                  <dd class="flex items-center gap-1 text-sm text-foreground tabular-nums">
                    {{ query.data.value.session }}
                    <SimpleTooltip
                      :tooltip="t('reports.sessions.detail.copy')"
                      as-child
                    >
                      <Button
                        variant="ghost"
                        size="toolbar"
                        class="text-muted"
                        :aria-label="t('reports.sessions.detail.copy')"
                        @click="copyId(query.data.value.session)"
                      >
                        <Icon name="lucide:copy" />
                      </Button>
                    </SimpleTooltip>
                  </dd>
                </div>
              </dl>
            </Panel>

            <Panel
              v-if="query.data.value.properties.length > 0"
              :title="t('reports.sessions.detail.properties')"
              flush
            >
              <dl>
                <div
                  v-for="property in query.data.value.properties"
                  :key="property.key"
                  class="grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-3 border-t border-divider px-4.5 py-2.5 text-sm"
                >
                  <dt class="truncate text-muted">{{ property.key }}</dt>
                  <dd class="truncate text-primary">{{ format.cellLabel('path', property.value) }}</dd>
                </div>
              </dl>
            </Panel>
          </div>
        </div>
      </div>
    </template>
  </SiteBoundary>
</template>
