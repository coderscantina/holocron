<script setup lang="ts" generic="T extends string">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'

/** One filter choice, shown as "Label: value" with the options in a radio menu. */
const props = defineProps<{
  label: string
  options: ReadonlyArray<{ value: T; label: string }>
}>()

const model = defineModel<T>({ required: true })

const current = computed(() => props.options.find((option) => option.value === model.value)?.label ?? '')

const select = (value: unknown) => {
  const option = props.options.find((entry) => entry.value === value)
  if (option) model.value = option.value
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        variant="fill"
        class="min-w-0 text-sm"
        :aria-label="`${label}: ${current}`"
      >
        <span class="font-normal text-muted">{{ label }}</span>
        <span class="truncate">{{ current }}</span>
        <Icon
          name="lucide:chevron-down"
          class="text-muted"
          aria-hidden="true"
        />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="start"
      class="w-64"
    >
      <DropdownMenuLabel class="text-xs font-medium text-muted">{{ label }}</DropdownMenuLabel>
      <DropdownMenuRadioGroup
        :model-value="model"
        @update:model-value="select"
      >
        <DropdownMenuRadioItem
          v-for="option in options"
          :key="option.value"
          :value="option.value"
        >
          {{ option.label }}
        </DropdownMenuRadioItem>
      </DropdownMenuRadioGroup>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
