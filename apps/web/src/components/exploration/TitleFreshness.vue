<script setup lang="ts">
import { useIntervalFn, useNow } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'
import type { ExploreResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import { useFreshnessFacts } from './freshness'
import { chartClock } from './trend'

/**
 * The page title row's freshness note: exact counts up to the application clock, or "Data
 * until" the watermark while analytics is stale. The routine facts of the result (dates,
 * timezone, correction, metric version) are its tooltip and part of its accessible text.
 * Nothing renders before a result arrives.
 */
const props = defineProps<{ result: ExploreResult | null }>()

const { t, locale } = useI18n()
const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })
const facts = useFreshnessFacts(() => props.result)

const text = computed(() => {
  const { result } = props
  if (!result) return null
  const clock = chartClock(now.value.getTime(), result)
  const time = new Intl.DateTimeFormat(locale.value, { timeZone: result.resolved.timezone, timeStyle: 'short' }).format(
    clock.watermark ?? clock.now
  )
  return t(clock.watermark === undefined ? 'explore.freshness.exact' : 'explore.freshness.stale', { time })
})

const detail = computed(() =>
  facts.value ? [facts.value.range, facts.value.timezone, facts.value.correction, facts.value.version].join(' · ') : ''
)
</script>

<template>
  <SimpleTooltip
    v-if="text"
    :tooltip="detail"
    class="flex cursor-help items-center gap-1.5 rounded-sm transition-colors hover:text-foreground"
  >
    <Icon
      name="lucide:clock"
      size="14"
      aria-hidden="true"
    />
    {{ text }}
    <span class="sr-only">. {{ detail }}</span>
  </SimpleTooltip>
</template>
