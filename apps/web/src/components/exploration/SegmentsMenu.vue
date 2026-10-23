<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
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
import { useSegments } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { ExplorationState } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { applySegment } from '~/services/exploration'

/** Applies the site's segments to the filter and saves the current filter as one. */
const props = defineProps<{
  state: ExplorationState
  /** The caller's role writes site definitions. */
  canSave: boolean
}>()
const emit = defineEmits<{
  commit: [state: ExplorationState]
  notice: [message: string, tone: 'success' | 'error']
}>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const segments = useSegments(() => props.state.site)

const open = ref(false)
const naming = ref(false)
const name = ref('')
const nameInput = ref<InstanceType<typeof Input> | null>(null)
const list = computed(() => segments.list.data.value?.segments ?? [])

const startNaming = () => {
  naming.value = true
  name.value = ''
  void nextTick(() => nameInput.value?.focus())
}

const save = async () => {
  const filter = props.state.filter
  const trimmed = name.value.trim()
  if (!filter || trimmed === '') return
  try {
    await segments.save.mutateAsync({ name: trimmed, filter })
    naming.value = false
    open.value = false
    emit('notice', t('explore.segments.saved', { name: trimmed }), 'success')
  } catch (error) {
    emit('notice', fieldMessage(error, 'name') ?? message(error), 'error')
  }
}
</script>

<template>
  <DropdownMenu v-model:open="open">
    <DropdownMenuTrigger as-child>
      <SimpleTooltip
        :tooltip="t('explore.segments.label')"
        as-child
      >
        <Button
          variant="ghost"
          size="icon"
          class="size-8 text-muted hover:text-primary max-sm:size-10"
          :aria-label="t('explore.segments.label')"
        >
          <Icon
            name="lucide:users-round"
            aria-hidden="true"
          />
        </Button>
      </SimpleTooltip>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="end"
      class="w-72"
    >
      <DropdownMenuLabel>{{ t('explore.segments.title') }}</DropdownMenuLabel>
      <p
        v-if="list.length === 0"
        class="px-2 py-3 text-sm text-muted"
      >
        {{ t('explore.segments.empty') }}
      </p>
      <DropdownMenuItem
        v-for="segment in list"
        :key="segment.id"
        class="group justify-between gap-2"
        @select="emit('commit', applySegment(state, segment.filter))"
      >
        <span class="truncate">{{ segment.name }}</span>
        <button
          v-if="canSave"
          type="button"
          class="rounded p-1 text-muted opacity-0 transition-opacity group-hover:opacity-100 group-data-highlighted:opacity-100 focus-visible:opacity-100 in-data-highlighted:text-accent-foreground"
          :aria-label="t('explore.segments.delete', { name: segment.name })"
          @click.stop.prevent="segments.remove.mutate(segment.id)"
        >
          <Icon
            name="lucide:trash-2"
            size="13"
          />
        </button>
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <p
        v-if="!canSave"
        class="px-2 py-2 text-xs text-muted"
      >
        {{ t('explore.segments.readOnly') }}
      </p>
      <div
        v-else-if="naming"
        class="p-1"
        @keydown.stop
      >
        <Input
          ref="nameInput"
          v-model="name"
          :placeholder="t('explore.segments.namePlaceholder')"
          :aria-label="t('explore.segments.namePlaceholder')"
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
            :disabled="name.trim() === '' || segments.save.isPending.value"
            @click="save"
          >
            {{ t('explore.views.saveAction') }}
          </Button>
        </div>
      </div>
      <DropdownMenuItem
        v-else
        :disabled="!state.filter"
        @select.prevent="startNaming"
      >
        <Icon
          name="lucide:plus"
          size="14"
          aria-hidden="true"
        />
        {{ t('explore.segments.save') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
