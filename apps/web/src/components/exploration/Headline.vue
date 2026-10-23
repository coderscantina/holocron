<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'
import { Translation } from 'vue-i18n'

import { SimpleTooltip } from '~/components/ui/tooltip'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { Annotation, ExplorationState, ExploreResult, Finding, Group, Measure, NamedMeasure } from '~/generated'
import { weekdayLabels } from '~/lib/exploration-labels'
import { headline, why } from '~/pages/overview/headline'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { changeTone, encodeState } from '~/services/exploration'

/**
 * The overview's sentence about one measure of a result, from the fixed grammar in
 * `pages/overview/headline.ts`. The value, the measure and the change open Explore with that
 * measure; the projected number names its band and basis in a tooltip. With `findings`, a muted
 * line names the change that explains most of today's difference; its value applies the
 * finding's filter through `filter`.
 */
const props = defineProps<{
  state: ExplorationState
  result: ExploreResult
  /** Index of the measure in `state.measures`. */
  measure: number
  /** The application clock, Unix milliseconds. */
  now: number
  /** The site's findings and the range's annotations; left out where findings do not describe the traffic on screen. */
  findings?: { list: readonly Finding[]; stale: boolean; annotations: readonly Annotation[] }
}>()
const emit = defineEmits<{ filter: [filter: Group] }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()

const nouns = {
  pageviews: 'overview.headline.nouns.pageviews',
  events: 'overview.headline.nouns.events',
  visitors: 'overview.headline.nouns.visitors',
  visitor_days: 'overview.headline.nouns.visitor_days',
  identified_people: 'overview.headline.nouns.identified_people',
  sessions: 'overview.headline.nouns.sessions',
  bounce_rate: 'overview.headline.nouns.bounce_rate',
  span_mean: 'overview.headline.nouns.span_mean',
  span_median: 'overview.headline.nouns.span_median',
  revenue_net: 'overview.headline.nouns.revenue_net',
  revenue_gross: 'overview.headline.nouns.revenue_gross',
  revenue_refunds: 'overview.headline.nouns.revenue_refunds',
  revenue_count: 'overview.headline.nouns.revenue_count',
  p50: 'overview.headline.nouns.p50',
  p75: 'overview.headline.nouns.p75',
  p95: 'overview.headline.nouns.p95',
} as const satisfies Record<NamedMeasure, MessageKey>

const selected = computed<Measure>(() => props.state.measures[props.measure] ?? 'pageviews')
const cut = computed(() => props.result.main)

const sentence = computed(() => {
  const index = props.measure
  const total = cut.value.totals[index]
  if (!total) return null
  return headline({
    total,
    comparison: cut.value.compare?.[index],
    projection: cut.value.projection?.[index],
    compare: props.state.compare,
    range: props.state.range,
    resolved: props.result.resolved,
    now: props.now,
  })
})

/** Visitors and visitor-days are one count; the noun follows the range (0003). */
const noun = computed(() => {
  const measure = selected.value
  if (typeof measure !== 'string') return format.measureLabel(measure)
  const named = measure === 'visitors' || measure === 'visitor_days' ? props.result.visitor_label : measure
  return t(nouns[named])
})

const value = computed(() => format.metric(cut.value.totals[props.measure]))

const exploreLink = computed(() => ({
  name: 'explore',
  params: { siteId: props.state.site },
  query: { s: encodeState({ ...props.state, measures: [selected.value], dimensions: [], sort: [] }) },
}))

const estimate = computed(() => {
  const current = sentence.value
  const projected = current?.estimate
  if (!current || !projected) return null
  const band = t('overview.headline.band', { low: numbers.integer(projected.low), high: numbers.integer(projected.high) })
  const basis = current.basis ? t(current.basis, { count: projected.basis_days }) : ''
  return { value: numbers.integer(projected.value), tooltip: `${band} ${basis}`.trim() }
})

/** The change reads in the success or destructive tone when it is good or bad news for the measure. */
const changeClass = computed(
  () => ({ good: 'text-success', bad: 'text-destructive', neutral: '' })[changeTone(selected.value, cut.value.compare?.[props.measure])]
)

const weekday = computed(() => {
  const day = sentence.value?.change?.weekday
  return day ? t(weekdayLabels[day]) : ''
})

const line = computed(() => {
  const source = props.findings
  const current = sentence.value
  if (!source || !current) return null
  const found = why({
    findings: source.list,
    stale: source.stale,
    annotations: source.annotations,
    compared: current.change !== null,
    resolved: props.result.resolved,
    now: props.now,
  })
  if (found?.kind !== 'mover') return found
  return {
    ...found,
    value: format.cellLabel(found.mover.dimension, found.mover.value),
    amount: new Intl.NumberFormat(locale.value, { signDisplay: 'exceptZero', maximumFractionDigits: 0 }).format(found.change),
    time: new Intl.DateTimeFormat(locale.value, { timeZone: props.result.resolved.timezone, timeStyle: 'short' }).format(found.since),
  }
})

const [DefineChange, ReuseChange] = createReusableTemplate()

const link =
  'rounded-sm underline decoration-border-strong decoration-1 underline-offset-[6px] transition-colors hover:decoration-current focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring'
</script>

<template>
  <section
    v-if="sentence"
    :aria-label="t('overview.headline.label')"
  >
    <DefineChange>
      <Translation
        v-if="sentence.change"
        :keypath="sentence.change.key"
        scope="global"
        tag="span"
      >
        <template #percent>
          <RouterLink
            :to="exploreLink"
            :class="[link, changeClass]"
          >
            {{ sentence.change.ratio === null ? '' : numbers.percent(sentence.change.ratio) }}
          </RouterLink>
        </template>
        <template #weekday>{{ weekday }}</template>
      </Translation>
    </DefineChange>

    <p class="max-w-[52rem] text-[1.875rem] leading-[1.22] font-semibold tracking-[-0.012em] text-balance text-primary">
      <Translation
        :keypath="sentence.lead"
        scope="global"
        tag="span"
      >
        <template #value>
          <RouterLink
            :to="exploreLink"
            :class="link"
            class="tabular-nums"
          >
            {{ value }}
          </RouterLink>
        </template>
        <template #measure>
          <RouterLink
            :to="exploreLink"
            :class="link"
            :aria-label="t('overview.headline.open', { measure: noun })"
          >
            {{ noun }}
          </RouterLink>
        </template>
        <template #range>{{ t(sentence.range.key, sentence.range.named) }}</template>
        <template #change><ReuseChange /></template>
      </Translation>
      <template v-if="sentence.pace && estimate">
        {{ ' ' }}
        <Translation
          :keypath="sentence.pace"
          scope="global"
          tag="span"
          class="font-[450] text-muted"
        >
          <template #projection>
            <SimpleTooltip
              :tooltip="estimate.tooltip"
              as-child
            >
              <button
                type="button"
                class="cursor-help rounded-sm font-[650] text-accent-text tabular-nums focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                :aria-label="`${t('overview.headline.estimate', { value: estimate.value })}. ${estimate.tooltip}`"
              >
                {{ estimate.value }}
              </button>
            </SimpleTooltip>
          </template>
          <template #change><ReuseChange /></template>
        </Translation>
      </template>
    </p>
    <p
      v-if="line"
      class="mt-2.5 text-[1rem] text-muted"
    >
      <Translation
        v-if="line.kind === 'mover'"
        :keypath="line.key"
        scope="global"
        tag="span"
      >
        <template #value>
          <button
            v-if="line.filter"
            type="button"
            :class="link"
            class="font-medium text-primary"
            :aria-label="t('overview.headline.why.filter', { value: line.value })"
            @click="line.filter && emit('filter', line.filter)"
          >
            {{ line.value }}
          </button>
          <span
            v-else
            class="font-medium text-primary"
          >
            {{ line.value }}
          </span>
        </template>
        <template #change>
          <span class="tabular-nums">{{ line.amount }}</span>
        </template>
        <template #time>{{ line.time }}</template>
        <template #annotation>{{ line.annotation?.text ?? '' }}</template>
      </Translation>
      <span v-else>{{ t(line.key) }}</span>
    </p>
    <p
      v-if="sentence.notes.length > 0"
      class="mt-2.5 text-[1rem] text-muted"
    >
      <span
        v-for="note in sentence.notes"
        :key="note"
      >
        {{ t(note) }}
      </span>
    </p>
  </section>
</template>
