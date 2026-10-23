<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ui/data-table/SavedViews.vue. Saves
// the complete exploration state through `explorations.save` instead of a flat table bag.
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeaderCombined } from '~/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Input } from '~/components/ui/input'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useSavedExplorations } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { ExplorationState, SavedExploration } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { pageOf } from '~/lib/state-pages'
import { sameState } from '~/services/exploration'

/**
 * The caller's saved views of this site. The trigger is the bookmark alone; it gains a check
 * mark, and its name becomes the view's, when the state on screen equals a saved one. Naming
 * happens inline; deleting asks once.
 */
const props = defineProps<{ state: ExplorationState }>()
const emit = defineEmits<{
  apply: [state: ExplorationState]
  notice: [message: string, tone: 'success' | 'error']
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const views = useSavedExplorations(() => props.state.site)

const open = ref(false)
const naming = ref(false)
const name = ref('')
const nameInput = ref<InstanceType<typeof Input> | null>(null)
const deleting = ref<SavedExploration | null>(null)

/** Views this page can draw: a retention view opens on the retention page, not in Explore. */
const list = computed(() =>
  (views.list.data.value?.explorations ?? []).filter((view) => pageOf(view.state) === pageOf(props.state))
)
const active = computed(() => list.value.find((view) => sameState(view.state, props.state)) ?? null)
const label = computed(() =>
  active.value ? t('explore.views.triggerActive', { name: active.value.name }) : t('explore.views.trigger')
)

const startNaming = () => {
  naming.value = true
  name.value = ''
  void nextTick(() => nameInput.value?.focus())
}

const save = async () => {
  const trimmed = name.value.trim()
  if (trimmed === '') return
  try {
    await views.save.mutateAsync({ name: trimmed, state: props.state })
    naming.value = false
    open.value = false
    emit('notice', t('explore.views.saved', { name: trimmed }), 'success')
  } catch (error) {
    emit('notice', message(error), 'error')
  }
}

const apply = (view: SavedExploration) => {
  emit('apply', view.state)
  open.value = false
}

const confirmDelete = async () => {
  const view = deleting.value
  if (!view) return
  try {
    await views.remove.mutateAsync(view.id)
  } catch (error) {
    emit('notice', message(error), 'error')
  }
  deleting.value = null
}
</script>

<template>
  <DropdownMenu v-model:open="open">
    <DropdownMenuTrigger as-child>
      <SimpleTooltip
        :tooltip="label"
        as-child
      >
        <Button
          variant="ghost"
          size="icon"
          class="size-7 text-muted hover:text-primary max-sm:size-10"
          :aria-label="label"
        >
          <Icon
            :name="active ? 'lucide:bookmark-check' : 'lucide:bookmark'"
            :class="active ? 'text-accent-text' : ''"
            aria-hidden="true"
          />
        </Button>
      </SimpleTooltip>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="end"
      class="w-72"
    >
      <DropdownMenuLabel>{{ t('explore.views.title') }}</DropdownMenuLabel>
      <p
        v-if="list.length === 0"
        class="px-2 py-3 text-sm text-muted"
      >
        {{ t('explore.views.empty') }}
      </p>
      <DropdownMenuItem
        v-for="view in list"
        :key="view.id"
        class="group justify-between gap-2"
        @select="apply(view)"
      >
        <span class="flex min-w-0 items-center gap-2">
          <Icon
            :name="active?.id === view.id ? 'lucide:check' : 'lucide:list-filter'"
            size="14"
            class="shrink-0 text-muted in-data-highlighted:text-accent-foreground"
            aria-hidden="true"
          />
          <span class="truncate">{{ view.name }}</span>
        </span>
        <button
          type="button"
          class="rounded p-1 text-muted opacity-0 transition-opacity group-hover:opacity-100 group-data-highlighted:opacity-100 focus-visible:opacity-100 hover:text-destructive in-data-highlighted:text-accent-foreground"
          :aria-label="t('explore.views.delete', { name: view.name })"
          @click.stop.prevent="(deleting = view), (open = false)"
        >
          <Icon
            name="lucide:trash-2"
            size="13"
          />
        </button>
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <div
        v-if="naming"
        class="p-1"
        @keydown.stop
      >
        <Input
          ref="nameInput"
          v-model="name"
          :placeholder="t('explore.views.namePlaceholder')"
          :aria-label="t('explore.views.namePlaceholder')"
          maxlength="100"
          class="h-8"
          @keydown.enter.prevent="save"
          @keydown.esc.prevent="naming = false"
        />
        <div class="mt-1 flex justify-end gap-1">
          <Button
            variant="ghost"
            size="xs"
            @click="naming = false"
          >
            {{ t('explore.views.cancel') }}
          </Button>
          <Button
            variant="accent"
            size="xs"
            :disabled="name.trim() === '' || views.save.isPending.value"
            @click="save"
          >
            {{ t('explore.views.saveAction') }}
          </Button>
        </div>
      </div>
      <DropdownMenuItem
        v-else
        @select.prevent="startNaming"
      >
        <Icon
          name="lucide:plus"
          size="14"
          aria-hidden="true"
        />
        {{ t('explore.views.save') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>

  <Dialog
    :open="deleting !== null"
    @update:open="(value) => !value && (deleting = null)"
  >
    <DialogContent class="sm:max-w-md">
      <DialogHeaderCombined
        :title="t('explore.views.deleteTitle')"
        :description="t('explore.views.deleteConfirm', { name: deleting?.name ?? '' })"
      />
      <DialogFooter>
        <Button
          variant="ghost"
          @click="deleting = null"
        >
          {{ t('explore.views.cancel') }}
        </Button>
        <Button
          variant="destructive"
          :disabled="views.remove.isPending.value"
          @click="confirmDelete"
        >
          {{ t('explore.views.confirm') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
