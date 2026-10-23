<script setup lang="ts">
import type { Summary } from '~/generated'
import { sitePace, summaryTime } from '~/lib/site-pace'
import { useI18n } from '~/plugins/i18n'

import { sparkline, type StatusSite } from './site-status'
import { usePaceWords } from './usePaceWords'

/**
 * One site in the workspace status list: today's views, the pace in words, findings, the
 * summary time and a 24-hour sparkline, all from the site's summary. The row is one link to the
 * site's overview; its accessible name is its visible text, the sparkline's text equivalent is
 * its description. A stale summary shows the name and its time, a missing one
 * "No status yet".
 */
const props = defineProps<{
  site: StatusSite
  summary?: Summary
  loading?: boolean
}>()

const { t, locale } = useI18n()

const pace = computed(() => sitePace(props.summary))
const fresh = computed(() => (props.summary && !props.summary.stale ? props.summary : null))

const words = usePaceWords()
const paceText = computed(() => (pace.value ? words.text(pace.value) : null))
const paceTone = computed(() => words.tone(pace.value))

const findings = computed(() => {
  const count = fresh.value?.finding_count ?? 0
  return count > 0 ? t('workspace.sites.findings', { count }) : t('workspace.sites.noFindings')
})

const time = computed(() => (props.summary ? summaryTime(props.summary.computed_at, locale.value) : null))

const status = computed(() => {
  if (props.loading) return null
  if (!props.summary) return t('workspace.sites.noStatus')
  return props.summary.stale ? t('workspace.sites.staleFrom', { time: time.value ?? '' }) : null
})

const chart = computed(() => (fresh.value ? sparkline(fresh.value) : null))
const chartText = computed(() => {
  const summary = fresh.value
  if (!summary) return ''
  const { today, typical } = summary.pageviews
  return typical === null
    ? t('workspace.sites.sparklineNoTypical', { today })
    : t('workspace.sites.sparkline', { today, typical })
})
const chartId = computed(() => `site-spark-${props.site.id}`)
</script>

<template>
  <RouterLink
    :to="{ name: 'overview', params: { siteId: site.id } }"
    :aria-describedby="chart ? chartId : undefined"
    class="grid min-h-table-row grid-cols-[minmax(0,1fr)_auto] items-center gap-x-4 gap-y-0.5 px-4 py-2.5 transition-colors hover:bg-fill focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring md:grid-cols-[minmax(0,2fr)_6rem_minmax(0,1.4fr)_7rem_8rem_6rem]"
  >
    <span class="grid min-w-0">
      <span class="truncate font-medium text-primary">{{ site.name }}</span>
      <span class="truncate text-xs text-muted">{{ site.domain }}</span>
    </span>

    <span class="col-start-2 row-start-1 flex items-center justify-end gap-3 md:contents">
      <span
        v-if="chart"
        class="hidden shrink-0 min-[480px]:block md:order-3 md:col-start-4"
        aria-hidden="true"
      >
        <svg
          viewBox="0 0 96 24"
          class="h-6 w-24 overflow-visible"
          preserveAspectRatio="none"
        >
          <polyline
            v-for="run in chart.typical"
            :key="run"
            :points="run"
            fill="none"
            stroke-width="1.5"
            stroke-dasharray="1 3"
            stroke-linecap="round"
            vector-effect="non-scaling-stroke"
            class="stroke-muted"
          />
          <polygon
            v-if="chart.area"
            :points="chart.area"
            class="fill-series-primary/15"
          />
          <polyline
            :points="chart.today"
            fill="none"
            stroke-width="2"
            stroke-linejoin="round"
            vector-effect="non-scaling-stroke"
            class="stroke-series-primary"
          />
        </svg>
      </span>
      <span
        v-if="fresh"
        :id="chartId"
        class="sr-only"
        aria-hidden="true"
      >
        {{ chartText }}
      </span>
      <span
        v-if="fresh"
        class="flex items-center gap-1.5 text-sm whitespace-nowrap md:order-4 md:col-start-5"
        :class="fresh.finding_count > 0 ? 'font-medium text-primary' : 'text-muted'"
      >
        <span
          v-if="fresh.finding_count > 0"
          class="size-2 shrink-0 rounded-full bg-accent"
          aria-hidden="true"
        />
        {{ findings }}
      </span>
    </span>

    <span
      v-if="fresh"
      class="col-span-2 flex flex-wrap items-baseline gap-x-2 text-sm md:contents"
    >
      <span class="tabular-nums text-primary md:order-1 md:col-start-2 md:text-right">
        {{ t('workspace.sites.views', { count: fresh.pageviews.today }) }}
      </span>
      <span
        v-if="pace?.kind !== 'quiet'"
        class="md:order-2 md:col-start-3"
        :class="paceTone"
      >
        {{ paceText }}
      </span>
      <span
        v-else
        class="md:order-2 md:col-start-3"
      />
      <span class="text-xs text-muted tabular-nums md:order-5 md:col-start-6 md:text-right">{{ time }}</span>
    </span>
    <span
      v-else
      class="col-span-2 text-sm text-muted md:col-span-4 md:col-start-2"
    >
      <template v-if="loading">&nbsp;</template>
      <template v-else>{{ status }}</template>
    </span>
  </RouterLink>
</template>
