<script setup lang="ts" generic="T extends string">
import { Checkbox } from '~/components/ui/checkbox'
import { useI18n } from '~/plugins/i18n'

/** A labelled set of checkboxes over a list of values, with an "all" toggle. */
const props = defineProps<{
  legend: string
  options: { value: T; label: string; mono?: boolean }[]
  error?: string | null
}>()
const model = defineModel<T[]>({ required: true })

const { t } = useI18n()
const id = useId()

const all = computed(() => props.options.length > 0 && props.options.every((option) => model.value.includes(option.value)))

const toggle = (value: T, on: boolean | 'indeterminate') => {
  model.value = on === true ? [...model.value, value] : model.value.filter((entry) => entry !== value)
}
const toggleAll = (on: boolean | 'indeterminate') => {
  const values = props.options.map((option) => option.value)
  model.value = on === true ? [...new Set([...model.value, ...values])] : model.value.filter((entry) => !values.includes(entry))
}
</script>

<template>
  <fieldset
    class="grid gap-2"
    :aria-describedby="error ? `${id}-error` : undefined"
  >
    <legend class="mb-1 text-sm font-medium text-primary">{{ legend }}</legend>
    <label class="flex w-fit items-center gap-2 text-xs text-muted">
      <Checkbox
        :model-value="all"
        @update:model-value="toggleAll"
      />
      {{ t('actions.selectAll') }}
    </label>
    <div class="grid max-h-56 gap-1.5 overflow-y-auto rounded-lg border border-border p-3 sm:grid-cols-2">
      <label
        v-for="option in options"
        :key="option.value"
        class="flex min-w-0 items-center gap-2 text-sm"
      >
        <Checkbox
          :model-value="model.includes(option.value)"
          @update:model-value="(on) => toggle(option.value, on)"
        />
        <span
          class="truncate"
          :class="option.mono && 'font-mono text-xs text-primary'"
        >
          {{ option.label }}
        </span>
      </label>
    </div>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </fieldset>
</template>
