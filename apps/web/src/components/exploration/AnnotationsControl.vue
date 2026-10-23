<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { Popover, PopoverContent, PopoverTrigger } from '~/components/ui/popover'
import { useAnnotations } from '~/composables/useDefinitions'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useI18n } from '~/plugins/i18n'

/**
 * Notes on the site's charts within the range on screen: listed with their time, added and
 * deleted by roles that write site definitions. The time is entered in the browser's timezone;
 * `at` prefills it, and the `trigger` slot replaces the button that opens the popover.
 */
const props = defineProps<{
  site: string
  range: { start: number; end: number }
  timezone: string
  canEdit: boolean
  at?: number
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const format = useExplorationFormat()
const annotations = useAnnotations(
  () => props.site,
  () => props.range
)

/** An instant as the value of a `datetime-local` input, in the browser's timezone. */
const inputValue = (ms: number) => {
  const date = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000)
  return date.toISOString().slice(0, 16)
}
const at = ref(props.at === undefined ? '' : inputValue(props.at))
const text = ref('')
const error = ref<string | null>(null)
const list = computed(() => annotations.list.data.value?.annotations ?? [])

const add = async () => {
  error.value = null
  const instant = Date.parse(at.value)
  if (Number.isNaN(instant) || text.value.trim() === '') return
  try {
    await annotations.create.mutateAsync({ at: instant, text: text.value })
    text.value = ''
  } catch (failure) {
    error.value = message(failure)
  }
}
const atId = useId()
const textId = useId()
</script>

<template>
  <Popover>
    <PopoverTrigger as-child>
      <slot name="trigger">
        <Button
          variant="ghost"
          size="sm"
          class="text-muted"
        >
          <Icon
            name="lucide:sticky-note"
            aria-hidden="true"
          />
          {{ t('explore.annotations.label') }}
          <span
            v-if="list.length > 0"
            class="tabular-nums"
          >
            ({{ list.length }})
          </span>
        </Button>
      </slot>
    </PopoverTrigger>
    <PopoverContent class="w-[min(22rem,calc(100vw-2*var(--shell-gutter-x)))]">
      <ul
        v-if="list.length > 0"
        class="mb-3 grid max-h-48 gap-1 overflow-y-auto"
      >
        <li
          v-for="annotation in list"
          :key="annotation.id"
          class="flex items-start justify-between gap-2 text-sm"
        >
          <span class="min-w-0">
            <span class="block text-xs text-muted tabular-nums">{{ format.date(annotation.at, timezone, true) }}</span>
            <span class="break-words">{{ annotation.text }}</span>
          </span>
          <Button
            v-if="canEdit"
            variant="ghost"
            size="toolbar"
            :aria-label="t('explore.annotations.delete', { text: annotation.text })"
            @click="annotations.remove.mutate(annotation.id)"
          >
            <Icon name="lucide:trash-2" />
          </Button>
        </li>
      </ul>
      <p
        v-else
        class="mb-3 text-sm text-muted"
      >
        {{ t('explore.annotations.empty') }}
      </p>
      <form
        v-if="canEdit"
        class="grid gap-2 border-t border-border pt-3"
        @submit.prevent="add"
      >
        <div class="grid gap-1">
          <Label :for="atId">{{ t('explore.annotations.at') }}</Label>
          <Input
            :id="atId"
            v-model="at"
            type="datetime-local"
            required
          />
        </div>
        <div class="grid gap-1">
          <Label :for="textId">{{ t('explore.annotations.text') }}</Label>
          <Input
            :id="textId"
            v-model="text"
            maxlength="500"
            required
          />
        </div>
        <p
          v-if="error"
          class="text-xs text-destructive"
          role="alert"
        >
          {{ error }}
        </p>
        <Button
          type="submit"
          size="sm"
          variant="accent"
          :disabled="annotations.create.isPending.value"
        >
          {{ t('explore.annotations.save') }}
        </Button>
      </form>
    </PopoverContent>
  </Popover>
</template>
