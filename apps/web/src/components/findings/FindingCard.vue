<script setup lang="ts">
import type { Finding, Group, Site } from '~/generated'

import FindingActions from './FindingActions.vue'
import type { FindingText } from './text'

/** A hairline card: kind and time in faint text, the fact as heading, the detail, then actions. */
defineProps<{ site: Site; finding: Finding; text: FindingText; time: string }>()
const emit = defineEmits<{ filter: [filter: Group] }>()
</script>

<template>
  <article
    class="flex flex-col gap-1.5 rounded-panel border border-hairline bg-card px-4.5 py-4"
    :aria-label="text.fact"
  >
    <p class="text-[0.875rem] text-muted">{{ text.kind }} · {{ time }}</p>
    <h3 class="text-[1.0625rem] leading-snug font-semibold text-primary">{{ text.fact }}</h3>
    <p class="text-sm text-muted">{{ text.detail }}</p>
    <FindingActions
      class="mt-2"
      :site="site"
      :finding="finding"
      :text="text"
      @filter="emit('filter', $event)"
    />
  </article>
</template>
