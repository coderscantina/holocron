<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { FilterField } from '~/composables/useFilterFields'
import type { ExplorationState, Field } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { canSplit, canonical, removeDimension, splitBy } from '~/services/exploration'

/** The state's dimensions as removable badges, and a menu to add one (three at most). */
const props = defineProps<{
  state: ExplorationState
  fields: FilterField[]
}>()
const emit = defineEmits<{ commit: [state: ExplorationState] }>()

const { t } = useI18n()
const format = useExplorationFormat()

/** Each field once, whatever value types it has. */
const choices = computed(() => {
  const seen = new Set<string>()
  return props.fields.flatMap((f) => {
    const key = canonical(f.field)
    if (seen.has(key) || !canSplit(props.state, f.field)) return []
    seen.add(key)
    return [f.field]
  })
})
const remove = (field: Field) => emit('commit', removeDimension(props.state, field))
</script>

<template>
  <div class="flex flex-wrap items-center gap-1.5">
    <span class="text-xs font-medium text-muted">{{ t('explore.dimensionsPicker') }}</span>
    <Badge
      v-for="field in state.dimensions"
      :key="canonical(field)"
      variant="secondary"
      size="sm"
      class="gap-1"
    >
      <span class="max-w-48 truncate">{{ format.fieldLabel(field) }}</span>
      <button
        type="button"
        class="-mr-1 inline-flex cursor-pointer rounded-sm hover:bg-current/10 focus-visible:outline-2 focus-visible:outline-ring"
        :aria-label="t('explore.removeDimension', { field: format.fieldLabel(field) })"
        @click="remove(field)"
      >
        <Icon
          name="lucide:x"
          size="12"
        />
      </button>
    </Badge>
    <span
      v-if="state.dimensions.length === 0"
      class="text-xs text-muted"
    >
      {{ t('explore.noDimensions') }}
    </span>
    <DropdownMenu v-if="choices.length > 0">
      <DropdownMenuTrigger as-child>
        <Button
          variant="ghost"
          size="xs"
        >
          <Icon
            name="lucide:plus"
            aria-hidden="true"
          />
          {{ t('explore.addDimension') }}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        class="max-h-80 w-60 overflow-y-auto"
      >
        <DropdownMenuLabel>{{ t('explore.dimensionsPicker') }}</DropdownMenuLabel>
        <DropdownMenuItem
          v-for="field in choices"
          :key="canonical(field)"
          @select="emit('commit', splitBy(state, field))"
        >
          {{ format.fieldLabel(field) }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
</template>
