<script setup lang="ts">
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Field } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { canonical } from '~/services/exploration'

/** The menu of fields a split can take; the trigger is the slot. Each field is listed once. */
const props = defineProps<{ fields: Field[] }>()
const emit = defineEmits<{ pick: [field: Field] }>()

const { t } = useI18n()
const format = useExplorationFormat()

const choices = computed(() => {
  const seen = new Set<string>()
  return props.fields.filter((field) => {
    const key = canonical(field)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
})
</script>

<template>
  <DropdownMenu v-if="choices.length > 0">
    <DropdownMenuTrigger as-child>
      <slot />
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="start"
      class="max-h-80 w-60 overflow-y-auto"
    >
      <DropdownMenuLabel>{{ t('explore.dimensionsPicker') }}</DropdownMenuLabel>
      <DropdownMenuItem
        v-for="field in choices"
        :key="canonical(field)"
        @select="emit('pick', field)"
      >
        {{ format.fieldLabel(field) }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
