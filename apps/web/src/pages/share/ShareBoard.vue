<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'

import { sharedViewResource } from '~/api/resources/sharing'
import { Card as Panel } from '~/components/ui/card'
import { EmptyState } from '~/components/ui/empty-state'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { Share } from '~/composables/useSharedView'
import type { Card, SharedView } from '~/generated'
import BoardCard from '~/pages/boards/BoardCard.vue'
import { useI18n } from '~/plugins/i18n'

import RecordingsLink from './RecordingsLink.vue'
import ShareHeader from './ShareHeader.vue'

/**
 * A shared board: its cards computed through the share. A card whose site the share does not
 * reach comes without a state and shows as unavailable.
 */
const props = defineProps<{ share: Share; view: SharedView; boardId: string }>()

const { t } = useI18n()
const { message } = useErrorMessage()

const query = useQuery({
  queryKey: ['share', props.share.scope.token, 'boards.get', props.boardId] as const,
  queryFn: ({ signal }) => sharedViewResource.board(props.share.scope.client, props.boardId, signal),
  staleTime: Infinity,
})
watch(() => query.error.value, props.share.check)

const board = computed(() => query.data.value?.board ?? null)
const siteName = (card: Card) => props.view.sites.find((site) => site.id === card.state?.site)?.name ?? null
</script>

<template>
  <ShareHeader
    :view="view"
    :title="board?.name"
  >
    <RecordingsLink v-if="view.recordings" />
  </ShareHeader>
  <div
    v-if="query.isPending.value"
    class="grid gap-4 lg:grid-cols-2"
    aria-busy="true"
    :aria-label="t('states.loading')"
  >
    <Skeleton class="h-48 w-full" />
    <Skeleton class="h-48 w-full" />
  </div>
  <EmptyState
    v-else-if="query.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="message(query.error.value)"
  />
  <EmptyState
    v-else-if="board && board.cards.length === 0"
    icon="lucide:layout-grid"
    :title="t('boards.emptyBoard')"
  />
  <ul
    v-else-if="board"
    class="grid gap-4 lg:grid-cols-2"
    :aria-label="board.name"
  >
    <li
      v-for="(card, index) in board.cards"
      :key="card.id"
      class="min-w-0"
    >
      <BoardCard
        v-if="card.state"
        :card="card"
        :pinned="card.state"
        :override="null"
        :site-name="siteName(card)"
        :can-edit="false"
        :first="index === 0"
        :last="index === board.cards.length - 1"
        :share="share.scope"
      />
      <Panel
        v-else
        class="grid gap-1 p-4"
      >
        <h2 class="truncate font-semibold text-primary">{{ card.title }}</h2>
        <p class="text-sm text-muted">{{ t('boards.cardUnavailable') }}</p>
      </Panel>
    </li>
  </ul>
</template>
