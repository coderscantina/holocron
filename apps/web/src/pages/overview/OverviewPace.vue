<script setup lang="ts">
import { useNow } from '@vueuse/core'

import PageHeader from '~/components/PageHeader.vue'
import { chartClock } from '~/components/exploration/trend'
import { listState, openFinding } from '~/components/findings/actions'
import { useFindingText } from '~/components/findings/useFindingText'
import Icon from '~/components/Icon.vue'
import StageView from '~/components/stage/StageView.vue'
import { Alert } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplore } from '~/composables/useExplore'
import { useFormat } from '~/composables/useFormat'
import { useSiteFindings } from '~/composables/useSiteFindings'
import type { ExplorationState, Site } from '~/generated'
import { FINDINGS_MAX } from '~/generated/limits'
import { dayPace, type PaceHour } from '~/lib/day-pace'
import { useI18n } from '~/plugins/i18n'
import { encodeState, include, overviewState, splitBy } from '~/services/exploration'

import PaceCurve from './PaceCurve.vue'

/**
 * The overview's default view: today's pageviews so far as a running total against a typical
 * weekday, with the projected end of the day, beside the figure, a one-line verdict and the
 * site's findings. Always today and all traffic; the Details view keeps ranges and filters. It
 * reads again whenever the site's summary moves, which is also when findings change.
 */
const props = defineProps<{ site: Site }>()

const { t, locale } = useI18n()
const format = useFormat()
const { message } = useErrorMessage()

const SHOWN = 3

const state = computed<ExplorationState>(() => ({ ...overviewState(props.site.id, props.site.timezone), measures: ['pageviews'] }))
const explore = useExplore(state)
const result = computed(() => explore.query.data.value ?? null)

const findings = useSiteFindings(() => props.site.id, FINDINGS_MAX)
const words = useFindingText(() => props.site)
watch(findings.computedAt, (next, previous) => {
  if (previous !== null && next !== previous) explore.refresh()
})
const now = useNow({ interval: 60_000 })
const listing = computed(() => listState(findings.computedAt.value, now.value.getTime()))
const shown = computed(() => findings.findings.value.slice(0, SHOWN).map((finding) => ({ finding, text: words.text(finding) })))

/** Counted up to the read, or to the watermark while analytics is behind. */
const pace = computed(() => {
  const answer = result.value
  if (!answer) return null
  const clock = chartClock(explore.query.dataUpdatedAt.value, answer)
  return dayPace(answer, 0, clock.watermark ?? clock.now)
})

const weekday = computed(() =>
  new Intl.DateTimeFormat(locale.value, { weekday: 'long', timeZone: props.site.timezone }).format(pace.value?.now ?? now.value)
)
const usual = computed(() => t('overview.pace.usual', { weekday: weekday.value }))
const clockTime = (ms: number) =>
  new Intl.DateTimeFormat(locale.value, { timeZone: props.site.timezone, hour: '2-digit', minute: '2-digit' }).format(ms)

const verdict = computed(() => {
  const current = pace.value
  if (!current) return ''
  const named = { weekday: weekday.value }
  switch (current.pace.kind) {
    case 'new':
      return t('overview.pace.verdict.new', named)
    case 'quiet':
      return t('overview.pace.verdict.quiet', named)
    case 'usual':
      return t('overview.pace.verdict.usual', named)
    case 'above':
    case 'below':
      return t(`overview.pace.verdict.${current.pace.kind}`, { ...named, percent: format.percent(Math.abs(current.pace.ratio)) })
  }
  return ''
})

/** An hour of today in Explore: its pages, filtered to that local hour. */
const hourLink = (hour: PaceHour) => {
  const base: ExplorationState = {
    ...overviewState(props.site.id, props.site.timezone),
    measures: ['pageviews', 'visitor_days'],
    sort: [{ by: 'pageviews', dir: 'desc' }],
  }
  const next = splitBy(include(base, 'hour', { t: 'integer', v: hour.hour }), 'path')
  return { name: 'explore', params: { siteId: props.site.id }, query: { s: encodeState(next) } }
}
</script>

<template>
  <PageHeader
    :title="t('nav.overview')"
    :description="site.domain"
  >
    <template #eyebrow>
      <slot name="context" />
    </template>
  </PageHeader>

  <Alert
    v-if="explore.query.isError.value && !result"
    color="destructive"
    role="alert"
    class="mb-4"
  >
    <p class="text-sm">{{ message(explore.query.error.value) }}</p>
  </Alert>

  <StageView :label="t('overview.pace.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <template v-if="pace">
          <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.integer(pace.today) }}</p>
          <p class="mt-3 flex items-center gap-2 text-[0.9375rem] text-muted">
            <span
              class="h-0.5 w-3.5 rounded-full bg-accent"
              aria-hidden="true"
            />
            {{ t('overview.pace.figure', { time: clockTime(pace.now) }) }}
          </p>
          <p class="mt-6 text-lg leading-snug font-semibold text-primary">{{ verdict }}</p>
          <p
            v-if="pace.projection"
            class="mt-2 text-[0.9375rem] text-muted"
          >
            {{
              t('overview.pace.projection', {
                value: format.integer(pace.projection.value),
                low: format.integer(pace.projection.low),
                high: format.integer(pace.projection.high),
              })
            }}
          </p>
        </template>
        <template v-else>
          <Skeleton class="h-13 w-40" />
          <Skeleton class="mt-3 h-5 w-48" />
          <Skeleton class="mt-6 h-6 w-full" />
        </template>
      </div>

      <h2 class="pt-5 pb-2 text-sm font-medium text-muted">{{ t('findings.label') }}</h2>
      <Skeleton
        v-if="findings.query.isPending.value"
        class="h-14 w-full"
      />
      <p
        v-else-if="listing !== 'current'"
        class="text-[0.9375rem] text-muted"
      >
        {{ listing === 'pending' ? t('findings.pending') : t('findings.behind', { time: words.clock(findings.computedAt.value ?? 0) }) }}
      </p>
      <p
        v-else-if="findings.findings.value.length === 0"
        class="text-[0.9375rem] text-muted"
      >
        {{ t('findings.empty') }}
      </p>
      <ul
        v-else
        class="-mx-2 flex flex-col"
      >
        <li
          v-for="({ finding, text }, index) in shown"
          :key="`${finding.rule}:${finding.rank}`"
        >
          <RouterLink
            :to="openFinding(site.id, finding, text.known)"
            class="flex gap-3 rounded-control px-2 py-2.5 hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
          >
            <Icon
              name="lucide:sparkles"
              size="16"
              class="mt-0.5 shrink-0"
              :class="index === 0 ? 'text-accent-text' : 'text-muted'"
              aria-hidden="true"
            />
            <span class="flex min-w-0 flex-col gap-0.5">
              <span class="text-[0.9375rem] font-medium text-primary">{{ text.fact }}</span>
              <span class="truncate text-sm text-muted">{{ text.detail }}</span>
            </span>
          </RouterLink>
        </li>
      </ul>
      <p
        v-if="listing === 'current' && findings.findings.value.length > SHOWN"
        class="pt-2 text-sm text-muted"
      >
        {{ t('overview.pace.moreFindings', { count: findings.findings.value.length - SHOWN }) }}
      </p>
    </template>

    <PaceCurve
      v-if="pace"
      :pace="pace"
      :timezone="site.timezone"
      :usual="usual"
      :link="hourLink"
    />
    <Skeleton
      v-else
      class="size-full min-h-[22rem]"
    />
  </StageView>
</template>
