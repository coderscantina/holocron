<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import RangeMenu from '~/components/exploration/RangeMenu.vue'
import { ApiError } from '~/api'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeaderCombined } from '~/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { useBoard } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSiteContext } from '~/composables/useSiteContext'
import type { Card, Range } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import BoardCard from './BoardCard.vue'

/**
 * One board. Cards render their pinned states; an optional board-wide range in the URL
 * overrides every card's range and each card shows that it does. Order changes and removals
 * send the whole card list with the revision the board was read at.
 */
const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { message } = useErrorMessage()
const context = useSiteContext()

const boardId = computed(() => String(route.params.boardId))
const { query, update, remove } = useBoard(boardId)
const board = computed(() => query.data.value?.board ?? null)
usePageMeta(() => board.value?.name ?? t('boards.title'))

const isRange = (value: unknown): value is Range =>
  typeof value === 'object' && value !== null && 'kind' in value && (value.kind === 'relative' || value.kind === 'days' || value.kind === 'all')

/** The board-wide range from `?range=`, or none. */
const override = computed<Range | null>(() => {
  const raw = route.query.range
  if (typeof raw !== 'string') return null
  try {
    const parsed: unknown = JSON.parse(raw)
    return isRange(parsed) ? parsed : null
  } catch {
    return null
  }
})
const setOverride = (range: Range | null) =>
  router.push({ query: { ...route.query, range: range ? JSON.stringify(range) : undefined } })

const siteName = (card: Card) =>
  context.workspaces.value.flatMap((w) => w.sites).find((site) => site.id === card.state?.site)?.name ?? null

const error = ref<string | null>(null)
const save = async (cards: Card[], name = board.value?.name ?? '') => {
  error.value = null
  try {
    await update.mutateAsync({
      name,
      cards: cards.map((card) => ({ id: card.id, title: card.title })),
      expected_revision: query.data.value?.revision,
    })
  } catch (failure) {
    error.value = message(failure)
  }
}
const move = (index: number, delta: number) => {
  const cards = [...(board.value?.cards ?? [])]
  const [card] = cards.splice(index, 1)
  if (!card) return
  cards.splice(index + delta, 0, card)
  void save(cards)
}
const removeCard = (index: number) => void save((board.value?.cards ?? []).filter((_, i) => i !== index))

const renaming = ref(false)
const name = ref('')
const startRename = () => {
  name.value = board.value?.name ?? ''
  renaming.value = true
}
const rename = async () => {
  await save(board.value?.cards ?? [], name.value)
  renaming.value = false
}
const deleting = ref(false)
const deleteBoard = async () => {
  try {
    await remove.mutateAsync()
    await router.push({ name: 'boards' })
  } catch (failure) {
    error.value = message(failure)
  }
}
const unavailable = computed(() => query.error.value instanceof ApiError && query.error.value.code === 'not_found')
</script>

<template>
  <Skeleton
    v-if="query.isPending.value"
    class="h-10 w-64"
  />
  <EmptyState
    v-else-if="unavailable || !board"
    icon="lucide:shield-question"
    :title="t('boards.unavailable')"
    :description="t('boards.unavailableHint')"
    class="mx-auto max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'boards' }">{{ t('boards.title') }}</RouterLink>
    </Button>
  </EmptyState>
  <template v-else>
    <PageHeader
      :title="board.name"
      :description="t('boards.cards', { count: board.cards.length })"
    >
      <template #actions>
        <RangeMenu
          :range="override ?? { kind: 'relative', unit: 'day', count: 30 }"
          @change="setOverride"
        />
        <Button
          v-if="override"
          variant="ghost"
          size="sm"
          @click="setOverride(null)"
        >
          {{ t('boards.ownRange') }}
        </Button>
        <DropdownMenu v-if="board.can_edit">
          <DropdownMenuTrigger as-child>
            <Button
              variant="fill"
              size="icon"
              :aria-label="t('boards.actions')"
            >
              <Icon
                name="lucide:ellipsis"
                aria-hidden="true"
              />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem @select="startRename">
              <Icon
                name="lucide:pencil"
                aria-hidden="true"
              />
              {{ t('boards.rename') }}
            </DropdownMenuItem>
            <DropdownMenuItem
              class="text-destructive"
              @select="deleting = true"
            >
              <Icon
                name="lucide:trash-2"
                aria-hidden="true"
              />
              {{ t('boards.delete') }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </template>
    </PageHeader>
    <p
      v-if="override"
      class="mb-3 text-sm text-warning"
      role="status"
    >
      {{ t('boards.overridden') }}
    </p>
    <p
      v-if="!board.can_edit"
      class="mb-3 text-sm text-muted"
    >
      {{ t('boards.readOnly') }}
    </p>
    <Alert
      v-if="error"
      color="destructive"
      role="alert"
      class="mb-3"
    >
      <p class="text-sm">{{ error }}</p>
    </Alert>
    <EmptyState
      v-if="board.cards.length === 0"
      icon="lucide:pin"
      :title="t('boards.emptyBoard')"
      :description="t('boards.emptyBoardHint')"
      class="mx-auto max-w-lg"
    />
    <ul
      v-else
      class="grid gap-4 lg:grid-cols-2"
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
          :override="override"
          :site-name="siteName(card)"
          :can-edit="board.can_edit"
          :first="index === 0"
          :last="index === board.cards.length - 1"
          @move="(delta) => move(index, delta)"
          @remove="removeCard(index)"
        />
        <Panel
          v-else
          :title="card.title"
          class="h-full"
        >
          <p class="text-sm text-muted">{{ t('boards.cardUnavailable') }}</p>
        </Panel>
      </li>
    </ul>

    <Dialog v-model:open="renaming">
      <DialogContent class="sm:max-w-sm">
        <DialogHeaderCombined :title="t('boards.rename')" />
        <form
          class="grid gap-3"
          @submit.prevent="rename"
        >
          <Input
            v-model="name"
            :aria-label="t('boards.namePlaceholder')"
            maxlength="100"
            required
          />
          <DialogFooter>
            <Button
              type="submit"
              variant="accent"
            >
              {{ t('boards.save') }}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
    <Dialog v-model:open="deleting">
      <DialogContent class="sm:max-w-md">
        <DialogHeaderCombined
          :title="t('boards.delete')"
          :description="t('boards.deleteConfirm', { name: board.name })"
        />
        <DialogFooter>
          <Button
            variant="ghost"
            @click="deleting = false"
          >
            {{ t('boards.cancel') }}
          </Button>
          <Button
            variant="destructive"
            @click="deleteBoard"
          >
            {{ t('boards.delete') }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </template>
</template>
