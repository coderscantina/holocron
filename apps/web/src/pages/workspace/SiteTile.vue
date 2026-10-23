<script setup lang="ts">
import { useFormat } from '~/composables/useFormat'
import type { Summary } from '~/generated'
import { sitePace, summaryTime } from '~/lib/site-pace'
import { useI18n } from '~/plugins/i18n'

import { activeThisHour, sparkline, type StatusSite } from './site-status'
import { usePaceWords } from './usePaceWords'

/**
 * One site on the workspace tiles: name, findings dot, today's views with the pace in words, and
 * the 24-hour sparkline of today against a typical day. A site with visitors in the current
 * hour breathes softly (not under reduced motion). The tile is one link to the site's
 * overview. `large` is the type size of the full screen wall.
 */
const props = defineProps<{
  site: StatusSite
  summary?: Summary
  loading?: boolean
  large?: boolean
}>()

const { t, locale } = useI18n()
const format = useFormat()
const words = usePaceWords()

const fresh = computed(() => (props.summary && !props.summary.stale ? props.summary : null))
const pace = computed(() => sitePace(props.summary))
const live = computed(() => activeThisHour(props.summary))
const findings = computed(() => fresh.value?.finding_count ?? 0)

const status = computed(() => {
  if (props.loading) return null
  if (!props.summary) return t('workspace.sites.noStatus')
  return t('workspace.sites.staleFrom', { time: summaryTime(props.summary.computed_at, locale.value) })
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
const chartId = computed(() => `site-tile-spark-${props.site.id}`)
</script>

<template>
  <RouterLink
    :to="{ name: 'overview', params: { siteId: site.id } }"
    :aria-describedby="chart ? chartId : undefined"
    class="flex h-full min-w-0 flex-col rounded-panel border border-hairline bg-card transition-colors hover:border-border-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    :class="[large ? 'gap-5 p-7' : 'gap-4 p-5', live ? 'site-tile-live' : '']"
  >
    <span class="flex items-start gap-3">
      <span class="grid min-w-0 flex-1">
        <span
          class="flex min-w-0 items-center gap-2 font-semibold text-primary"
          :class="large ? 'text-2xl' : 'text-base'"
        >
          <span class="truncate">{{ site.name }}</span>
          <span
            v-if="live"
            class="size-2 shrink-0 rounded-full bg-success"
            aria-hidden="true"
          />
          <span
            v-if="live"
            class="sr-only"
          >
            {{ t('workspace.sites.tiles.live') }}
          </span>
        </span>
        <span
          class="truncate text-muted"
          :class="large ? 'text-base' : 'text-sm'"
        >
          {{ site.domain }}
        </span>
      </span>
      <span
        v-if="findings > 0"
        class="flex shrink-0 items-center gap-1.5 font-medium whitespace-nowrap text-primary"
        :class="large ? 'text-base' : 'text-sm'"
      >
        <span
          class="size-2.5 rounded-full bg-accent"
          aria-hidden="true"
        />
        {{ t('workspace.sites.findings', { count: findings }) }}
      </span>
    </span>

    <span
      v-if="fresh"
      class="mt-auto grid"
    >
      <span
        class="leading-none font-bold tracking-[-0.025em] text-primary tabular-nums"
        :class="large ? 'text-[4rem]' : 'text-[2rem]'"
      >
        {{ format.integer(fresh.pageviews.today) }}
      </span>
      <span
        class="mt-2 flex flex-wrap gap-x-2 text-muted"
        :class="large ? 'text-lg' : 'text-sm'"
      >
        {{ t('workspace.sites.tiles.viewsToday', { count: fresh.pageviews.today }) }}
        <span
          v-if="pace && pace.kind !== 'quiet'"
          class="font-medium tabular-nums"
          :class="words.tone(pace)"
        >
          {{ words.text(pace) }}
        </span>
      </span>
    </span>
    <span
      v-else
      class="mt-auto text-sm text-muted"
    >
      <template v-if="loading">&nbsp;</template>
      <template v-else>{{ status }}</template>
    </span>

    <span
      v-if="chart"
      class="block"
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 96 24"
        class="w-full overflow-visible"
        :class="large ? 'h-16' : 'h-10'"
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
  </RouterLink>
</template>

<style scoped>
@media (prefers-reduced-motion: no-preference) {
  .site-tile-live:not(:hover) {
    animation: site-tile-breathe 4s ease-in-out infinite;
  }
}

@keyframes site-tile-breathe {
  50% {
    border-color: color-mix(in oklch, var(--success) 45%, var(--hairline));
    box-shadow: 0 0 0 3px color-mix(in oklch, var(--success) 10%, transparent);
  }
}
</style>
