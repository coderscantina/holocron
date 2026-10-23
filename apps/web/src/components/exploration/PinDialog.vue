<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeaderCombined } from '~/components/ui/dialog'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useBoards } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { ExplorationState } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** Pins the exact state to a board of the site's workspace, or to a new board named here. */
const props = defineProps<{
  state: ExplorationState
  workspace: string
  /** Default card title. */
  title: string
}>()
const emit = defineEmits<{ notice: [message: string, tone: 'success' | 'error'] }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const boards = useBoards(() => props.workspace)

const open = ref(false)
const board = ref('')
const newBoard = ref('')
const cardTitle = ref('')
const error = ref<string | null>(null)
const list = computed(() => boards.list.data.value?.boards ?? [])
const canEdit = computed(() => boards.list.data.value?.can_edit ?? false)

watch(open, (value) => {
  if (!value) return
  cardTitle.value = props.title
  board.value = list.value[0]?.id ?? ''
  newBoard.value = ''
  error.value = null
})

const pin = async () => {
  error.value = null
  try {
    let target = board.value
    let name = list.value.find((b) => b.id === target)?.name ?? ''
    if (!target) {
      const created = await boards.create.mutateAsync(newBoard.value)
      target = created.board.id
      name = created.board.name
    }
    await boards.pin.mutateAsync({ board: target, title: cardTitle.value, state: props.state })
    open.value = false
    emit('notice', t('explore.pin.pinned', { board: name }), 'success')
  } catch (failure) {
    error.value = message(failure)
  }
}
const titleId = useId()
const boardId = useId()
const newId = useId()
</script>

<template>
  <SimpleTooltip
    :tooltip="t('explore.pin.label')"
    as-child
  >
    <Button
      variant="ghost"
      size="icon"
      class="size-7 text-muted hover:text-primary max-sm:size-10"
      :aria-label="t('explore.pin.label')"
      @click="open = true"
    >
      <Icon
        name="lucide:pin"
        aria-hidden="true"
      />
    </Button>
  </SimpleTooltip>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md">
      <DialogHeaderCombined
        :title="t('explore.pin.title')"
        :description="t('explore.pin.description')"
      />
      <p
        v-if="boards.list.isSuccess.value && !canEdit"
        class="text-sm text-muted"
      >
        {{ t('explore.pin.readOnly') }}
      </p>
      <form
        v-else
        class="grid gap-3"
        @submit.prevent="pin"
      >
        <div class="grid gap-1.5">
          <Label :for="boardId">{{ t('explore.pin.board') }}</Label>
          <select
            :id="boardId"
            v-model="board"
            class="h-9 rounded-lg border border-input-border bg-input px-2 text-sm text-foreground focus-visible:border-ring focus-visible:outline-none"
          >
            <option
              v-for="item in list"
              :key="item.id"
              :value="item.id"
            >
              {{ item.name }}
            </option>
            <option value="">{{ t('explore.pin.newBoard') }}</option>
          </select>
        </div>
        <div
          v-if="!board"
          class="grid gap-1.5"
        >
          <Label :for="newId">{{ t('explore.pin.newBoard') }}</Label>
          <Input
            :id="newId"
            v-model="newBoard"
            maxlength="100"
            required
          />
          <p
            v-if="list.length === 0"
            class="text-xs text-muted"
          >
            {{ t('explore.pin.noBoards') }}
          </p>
        </div>
        <div class="grid gap-1.5">
          <Label :for="titleId">{{ t('explore.pin.cardTitle') }}</Label>
          <Input
            :id="titleId"
            v-model="cardTitle"
            maxlength="100"
            required
          />
        </div>
        <p
          v-if="error"
          class="text-sm text-destructive"
          role="alert"
        >
          {{ error }}
        </p>
        <DialogFooter>
          <Button
            type="submit"
            variant="accent"
            :disabled="boards.pin.isPending.value || boards.create.isPending.value"
          >
            {{ t('explore.pin.pin') }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
