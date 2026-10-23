<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import type { RetentionResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * What a retention result counts and what it cannot know: the consented population, its
 * coverage of all eligible events, the retained history cohort entry looks back to, and the
 * instant after which intervals are incomplete.
 */
const props = defineProps<{
  result: RetentionResult
  timezone: string
}>()

const { t, locale } = useI18n()

const integer = (value: number) => new Intl.NumberFormat(locale.value).format(value)
const coverage = computed(() => {
  const { with_identity, eligible_events } = props.result.coverage
  return t('retention.labels.coverage', {
    consented: integer(with_identity),
    eligible: integer(eligible_events),
    share:
      eligible_events === 0
        ? '–'
        : new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 1 }).format(
            with_identity / eligible_events
          ),
  })
})
const instant = (ms: number, withTime: boolean) =>
  new Intl.DateTimeFormat(locale.value, {
    timeZone: props.timezone,
    dateStyle: 'medium',
    ...(withTime ? { timeStyle: 'short' } : {}),
  }).format(ms)
</script>

<template>
  <ul
    class="grid gap-1.5 text-sm text-muted"
    :aria-label="t('retention.labels.label')"
  >
    <li class="flex items-start gap-2">
      <Icon
        name="lucide:user-check"
        size="15"
        class="mt-0.5 shrink-0"
        aria-hidden="true"
      />
      <span>
        <strong class="font-semibold text-primary">{{ t('retention.labels.population') }}</strong>
        {{ coverage }}
      </span>
    </li>
    <li
      v-if="result.history_start !== undefined"
      class="flex items-start gap-2"
    >
      <Icon
        name="lucide:archive"
        size="15"
        class="mt-0.5 shrink-0"
        aria-hidden="true"
      />
      <span>{{ t('retention.labels.history', { date: instant(result.history_start, false) }) }}</span>
    </li>
    <li class="flex items-start gap-2">
      <Icon
        name="lucide:hourglass"
        size="15"
        class="mt-0.5 shrink-0"
        aria-hidden="true"
      />
      <span>{{ t('retention.labels.incomplete', { time: instant(result.watermark, true) }) }}</span>
    </li>
  </ul>
</template>
