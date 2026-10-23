<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'
import type { Finding, Group, Site } from '~/generated'

import FindingActions from './FindingActions.vue'
import type { FindingText } from './text'

/**
 * One finding as a row of the findings panel: sparkle, the fact, the measured detail on one line
 * (the whole text is the group's accessible name and the tooltip of the text) and its actions.
 * The lead finding's sparkle takes the accent.
 */
defineProps<{ site: Site; finding: Finding; text: FindingText; lead?: boolean }>()
const emit = defineEmits<{ filter: [filter: Group] }>()
</script>

<template>
  <div
    role="group"
    :aria-label="`${text.fact}. ${text.detail}`"
    class="flex min-w-0 flex-col gap-2 px-4.5 py-3 md:min-h-14 md:flex-row md:items-center md:gap-3.5 md:py-2"
  >
    <SimpleTooltip
      as-child
      :tooltip="`${text.fact}. ${text.detail}`"
    >
      <div
        tabindex="0"
        class="flex min-w-0 flex-1 items-start gap-3"
      >
        <Icon
          name="lucide:sparkles"
          size="18"
          class="mt-0.5 shrink-0"
          :class="lead ? 'text-accent-text' : 'text-muted'"
          aria-hidden="true"
        />
        <span class="flex min-w-0 flex-col gap-0.5">
          <span class="font-semibold text-primary md:truncate">{{ text.fact }}</span>
          <span class="min-w-0 text-[0.875rem] text-muted md:truncate">{{ text.detail }}</span>
        </span>
      </div>
    </SimpleTooltip>
    <FindingActions
      class="md:ml-auto md:shrink-0"
      :site="site"
      :finding="finding"
      :text="text"
      @filter="emit('filter', $event)"
    />
  </div>
</template>
