<script setup lang="ts">
import type { Granularity, RetentionResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** A board card's view of a retention result: its population and the first averages. */
const props = defineProps<{
  result: RetentionResult
  granularity: Granularity
}>()

const { t, locale } = useI18n()

const profiles = computed(() => props.result.cohorts.reduce((sum, cohort) => sum + cohort.size, 0))
const averages = computed(() => props.result.averages.slice(0, 4))
const integer = (value: number) => new Intl.NumberFormat(locale.value).format(value)
const percent = (value: number) =>
  new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 1 }).format(value)
const unit = computed(() => (props.granularity === 'week' ? 'retention.table.week' : 'retention.table.day'))
</script>

<template>
  <div class="grid gap-2">
    <p class="text-xs text-muted">
      {{ t('retention.summary', { profiles: integer(profiles), cohorts: result.cohorts.length }) }}
    </p>
    <dl class="grid grid-cols-4 gap-2">
      <div
        v-for="(average, offset) in averages"
        :key="offset"
        class="min-w-0"
      >
        <dt class="truncate text-xs text-muted">{{ t(unit, { offset }) }}</dt>
        <dd class="text-lg font-semibold text-primary tabular-nums">
          {{ average.t === 'ratio' ? percent(average.v) : '–' }}
        </dd>
      </div>
    </dl>
  </div>
</template>
