<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Input } from '~/components/ui/input'
import { useI18n } from '~/plugins/i18n'
import { pageview, type NewCondition } from '~/services/reports'

import ChoiceGroup from './ChoiceGroup.vue'

/**
 * Adds a predicate in one step: a page path or an event name. The predicate's own filter bar
 * refines it afterwards with any field, operator and group.
 */
defineProps<{
  /** Names the action: "Add step", "Add goal condition". */
  action: string
  disabled?: boolean
}>()

const emit = defineEmits<{ add: [items: NewCondition[]] }>()

const { t } = useI18n()
const id = useId()

type Kind = 'page' | 'event'
const kind = ref<Kind>('page')
const value = ref('')

const submit = () => {
  const text = value.value.trim()
  if (text === '') return
  emit('add', kind.value === 'page' ? pageview(text) : [{ field: 'event_name', cmp: 'eq', value: { t: 'string', v: text } }])
  value.value = ''
}
</script>

<template>
  <form
    class="flex flex-wrap items-center gap-2"
    @submit.prevent="submit"
  >
    <ChoiceGroup
      v-model="kind"
      :label="t('reports.quickAdd.kind')"
      :options="[
        { value: 'page', label: t('reports.quickAdd.page') },
        { value: 'event', label: t('reports.quickAdd.event') },
      ]"
    />
    <label
      :for="id"
      class="sr-only"
    >
      {{ kind === 'page' ? t('reports.quickAdd.pagePlaceholder') : t('reports.quickAdd.eventPlaceholder') }}
    </label>
    <Input
      :id="id"
      v-model="value"
      class="h-8 w-56 max-sm:w-full"
      :placeholder="kind === 'page' ? t('reports.quickAdd.pagePlaceholder') : t('reports.quickAdd.eventPlaceholder')"
      :disabled="disabled"
    />
    <Button
      type="submit"
      variant="outline"
      size="sm"
      :disabled="disabled || value.trim() === ''"
    >
      <Icon
        name="lucide:plus"
        aria-hidden="true"
      />
      {{ action }}
    </Button>
  </form>
</template>
