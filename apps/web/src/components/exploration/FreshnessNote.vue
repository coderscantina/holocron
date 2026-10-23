<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import type { ExploreResult } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import { useFreshnessFacts } from './freshness'

/**
 * What a result covers and can claim. By default only what departs from the routine: days still
 * being recalculated, a range clipped at the retention limit. The routine facts
 * (dates, timezone, correction, metric version) live in the title row's freshness note; `full`
 * lists them here too, for screens without a title row.
 */
const props = defineProps<{ result: ExploreResult; full?: boolean }>()

const { t } = useI18n()
const facts = useFreshnessFacts(() => props.result)
</script>

<template>
  <ul
    v-if="facts"
    class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-muted empty:hidden"
    :aria-label="t('explore.freshness.label')"
  >
    <template v-if="full">
      <li class="flex items-center gap-1.5">
        <Icon
          name="lucide:calendar"
          size="14"
          aria-hidden="true"
        />
        {{ facts.range }}
      </li>
      <li class="flex items-center gap-1.5">
        <Icon
          name="lucide:globe"
          size="14"
          aria-hidden="true"
        />
        {{ facts.timezone }}
      </li>
    </template>
    <li
      v-if="full || facts.pending"
      class="flex items-center gap-1.5"
      :class="facts.pending ? 'font-medium text-warning' : ''"
    >
      <Icon
        :name="facts.pending ? 'lucide:loader' : 'lucide:circle-check'"
        size="14"
        aria-hidden="true"
      />
      {{ facts.correction }}
    </li>
    <li
      v-if="full"
      class="flex items-center gap-1.5"
    >
      <Icon
        name="lucide:sigma"
        size="14"
        aria-hidden="true"
      />
      {{ facts.version }}
    </li>
    <li
      v-if="facts.retention"
      class="flex items-center gap-1.5"
    >
      <Icon
        name="lucide:archive"
        size="14"
        aria-hidden="true"
      />
      {{ facts.retention }}
    </li>
  </ul>
</template>
