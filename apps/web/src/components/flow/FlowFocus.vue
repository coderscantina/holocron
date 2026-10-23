<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import type { ExplorationState } from '~/generated'
import { pageOf } from '~/lib/state-pages'
import { useI18n } from '~/plugins/i18n'
import { encodeState } from '~/services/exploration'

/**
 * What a flow is focused on, for the column beside it: the name, one line of figures, and
 * links to the sessions and events behind it when there is a filter for them.
 */
defineProps<{
  title: string
  summary: string
  sessions?: ExplorationState
  events?: ExplorationState
}>()

const emit = defineEmits<{ clear: [] }>()

const { t } = useI18n()

const link = (state: ExplorationState) => ({ name: pageOf(state), params: { siteId: state.site }, query: { s: encodeState(state) } })
</script>

<template>
  <div class="border-b border-hairline py-5">
    <div class="flex items-start justify-between gap-2">
      <p class="min-w-0 text-[0.9375rem] font-semibold break-words text-primary">{{ title }}</p>
      <Button
        variant="ghost"
        size="toolbar"
        class="-mt-1 shrink-0 text-muted"
        :aria-label="t('flow.clear')"
        @click="emit('clear')"
      >
        <Icon
          name="lucide:x"
          aria-hidden="true"
        />
      </Button>
    </div>
    <p class="mt-1 text-sm text-muted tabular-nums">{{ summary }}</p>
    <div
      v-if="sessions || events"
      class="mt-3 flex flex-wrap gap-2"
    >
      <Button
        v-if="sessions"
        variant="fill"
        size="xs"
        as-child
      >
        <RouterLink :to="link(sessions)">
          <Icon
            name="lucide:users"
            aria-hidden="true"
          />
          {{ t('reports.drill.sessions') }}
        </RouterLink>
      </Button>
      <Button
        v-if="events"
        variant="fill"
        size="xs"
        as-child
      >
        <RouterLink :to="link(events)">
          <Icon
            name="lucide:search"
            aria-hidden="true"
          />
          {{ t('reports.drill.events') }}
        </RouterLink>
      </Button>
    </div>
  </div>
</template>
