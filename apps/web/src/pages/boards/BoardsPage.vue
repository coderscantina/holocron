<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Button } from '~/components/ui/button'
import { Card } from '~/components/ui/card'
import { EmptyState } from '~/components/ui/empty-state'
import { Input } from '~/components/ui/input'
import { Skeleton } from '~/components/ui/skeleton'
import { useBoards } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSiteContext } from '~/composables/useSiteContext'
import { useI18n } from '~/plugins/i18n'

/** The boards of the workspace in the header context, or the first one on a direct load. */
const { t } = useI18n()
const { message } = useErrorMessage()
const router = useRouter()
const context = useSiteContext()
const workspace = computed(() => context.workspace.value ?? context.workspaces.value[0] ?? null)
const boards = useBoards(() => workspace.value?.id ?? null)
usePageMeta(() => t('boards.title'))

const name = ref('')
const error = ref<string | null>(null)
const create = async () => {
  error.value = null
  try {
    const created = await boards.create.mutateAsync(name.value)
    name.value = ''
    await router.push({ name: 'board', params: { boardId: created.board.id } })
  } catch (failure) {
    error.value = message(failure)
  }
}
const nameId = useId()
</script>

<template>
  <Skeleton
    v-if="context.loading.value"
    class="h-10 w-64"
  />
  <EmptyState
    v-else-if="!workspace"
    icon="lucide:layout-grid"
    :title="t('boards.noWorkspace')"
    class="mx-auto max-w-lg"
  />
  <template v-else>
    <PageHeader
      :title="t('boards.title')"
      :description="workspace.name"
    >
      <template
        v-if="boards.list.data.value?.can_edit"
        #actions
      >
        <form
          class="flex items-center gap-2"
          @submit.prevent="create"
        >
          <label
            :for="nameId"
            class="sr-only"
          >
            {{ t('boards.namePlaceholder') }}
          </label>
          <Input
            :id="nameId"
            v-model="name"
            :placeholder="t('boards.namePlaceholder')"
            maxlength="100"
            required
            class="h-8 w-48"
          />
          <Button
            type="submit"
            variant="accent"
            class="text-sm"
            :disabled="boards.create.isPending.value"
          >
            <Icon
              name="lucide:plus"
              aria-hidden="true"
            />
            {{ t('boards.createAction') }}
          </Button>
        </form>
      </template>
    </PageHeader>
    <p
      v-if="error"
      class="mb-3 text-sm text-destructive"
      role="alert"
    >
      {{ error }}
    </p>
    <div
      v-if="boards.list.isPending.value"
      class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3"
    >
      <Skeleton
        v-for="n in 3"
        :key="n"
        class="h-20"
      />
    </div>
    <EmptyState
      v-else-if="(boards.list.data.value?.boards.length ?? 0) === 0"
      icon="lucide:layout-grid"
      :title="t('boards.empty')"
      :description="t('boards.emptyHint')"
      class="mx-auto max-w-lg"
    />
    <ul
      v-else
      class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3"
    >
      <li
        v-for="board in boards.list.data.value?.boards"
        :key="board.id"
      >
        <RouterLink
          :to="{ name: 'board', params: { boardId: board.id } }"
          class="block rounded-panel focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          <Card class="gap-0.5 p-4.5 transition-colors hover:bg-fill/50">
            <p class="truncate font-semibold text-primary">{{ board.name }}</p>
            <p class="text-sm text-muted tabular-nums">{{ t('boards.cards', { count: board.cards }) }}</p>
          </Card>
        </RouterLink>
      </li>
    </ul>
  </template>
</template>
