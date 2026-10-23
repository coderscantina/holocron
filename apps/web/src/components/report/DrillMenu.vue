<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import type { ExplorationState } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { pageOf } from '~/lib/state-pages'
import { encodeState } from '~/services/exploration'

/**
 * The named action menu of one result row: open the sessions behind it, or its events in
 * Explore. Both are links to complete states, so they open in a new tab and survive Back.
 */
const props = defineProps<{
  /** The row's name, for the menu's accessible name. */
  label: string
  sessions?: ExplorationState
  events?: ExplorationState
}>()

const { t } = useI18n()

const link = (state: ExplorationState) => ({
  name: pageOf(state),
  params: { siteId: state.site },
  query: { s: encodeState(state) },
})
const links = computed(() => [
  ...(props.sessions ? [{ key: 'sessions', icon: 'lucide:users', text: t('reports.drill.sessions'), to: link(props.sessions) }] : []),
  ...(props.events ? [{ key: 'events', icon: 'lucide:search', text: t('reports.drill.events'), to: link(props.events) }] : []),
])
</script>

<template>
  <DropdownMenu v-if="links.length > 0">
    <DropdownMenuTrigger as-child>
      <Button
        variant="ghost"
        size="toolbar"
        class="text-muted"
        :aria-label="t('reports.drill.actionsFor', { value: label })"
      >
        <Icon
          name="lucide:ellipsis"
          aria-hidden="true"
        />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuLabel class="max-w-64 truncate text-xs text-muted">{{ label }}</DropdownMenuLabel>
      <DropdownMenuItem
        v-for="item in links"
        :key="item.key"
        as-child
      >
        <RouterLink :to="item.to">
          <Icon
            :name="item.icon"
            aria-hidden="true"
          />
          {{ item.text }}
        </RouterLink>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
