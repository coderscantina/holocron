<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/FormField.vue, without field presence
// and the password reveal.
import type { HTMLAttributes } from 'vue'

import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'

const props = defineProps<{
  id: string
  label: string
  type?: string
  autocomplete?: string
  inputmode?: HTMLAttributes['inputmode']
  error?: string | null
  /** Hint under the field. Say what the rule is, not that there is one. */
  description?: string
  required?: boolean
  readonly?: boolean
}>()

const model = defineModel<string>({ default: '' })

/** The hint and the error are both announced, so the broken rule is never left unsaid. */
const describedBy = computed(() => {
  const ids = [props.description ? `${props.id}-description` : null, props.error ? `${props.id}-error` : null]
  const present = ids.filter((id) => id !== null)
  return present.length > 0 ? present.join(' ') : undefined
})
</script>

<template>
  <div class="grid gap-1.5">
    <Label :for="id">{{ label }}</Label>
    <Input
      :id="id"
      v-model="model"
      :type="type ?? 'text'"
      :autocomplete="autocomplete"
      :inputmode="inputmode"
      :required="required"
      :readonly="readonly"
      :aria-invalid="error ? 'true' : undefined"
      :aria-describedby="describedBy"
    />
    <p
      v-if="description"
      :id="`${id}-description`"
      class="text-xs text-muted"
    >
      {{ description }}
    </p>
    <p
      v-if="error"
      :id="`${id}-error`"
      class="text-xs font-medium text-destructive"
    >
      {{ error }}
    </p>
  </div>
</template>
