<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { FilterField } from '~/composables/useFilterFields'
import type { ExplorationState, Measure, Stat } from '~/generated'
import { EXPLORATION_MEASURES_MAX } from '~/generated/limits'
import { namedMeasures } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'
import { canonical, setMeasures } from '~/services/exploration'

/** Chooses the state's measures: the named formulas and statistics over numeric event properties. */
const props = defineProps<{
  state: ExplorationState
  fields: FilterField[]
}>()
const emit = defineEmits<{ commit: [state: ExplorationState] }>()

const { t } = useI18n()
const format = useExplorationFormat()

const stats: Stat[] = ['sum', 'mean', 'min', 'max', 'count']
const numeric = computed(() =>
  props.fields.flatMap((f) => (f.kind === 'number' && typeof f.field !== 'string' && f.field.scope === 'event' ? [f.field] : []))
)
const chosen = (measure: Measure) => props.state.measures.some((m) => canonical(m) === canonical(measure))
const full = computed(() => props.state.measures.length >= EXPLORATION_MEASURES_MAX)

const toggle = (measure: Measure, on: boolean | 'indeterminate') => {
  const others = props.state.measures.filter((m) => canonical(m) !== canonical(measure))
  emit('commit', setMeasures(props.state, on === true ? [...props.state.measures, measure] : others))
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        variant="outline"
        size="sm"
      >
        <Icon
          name="lucide:sigma"
          aria-hidden="true"
        />
        {{ t('explore.measuresPicker') }}
        <span class="text-muted tabular-nums">{{ state.measures.length }}</span>
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent
      align="start"
      class="w-64"
    >
      <DropdownMenuLabel>{{ t('explore.measuresPicker') }}</DropdownMenuLabel>
      <DropdownMenuCheckboxItem
        v-for="measure in namedMeasures"
        :key="measure"
        :model-value="chosen(measure)"
        :disabled="(chosen(measure) && state.measures.length === 1) || (!chosen(measure) && full)"
        @update:model-value="(on) => toggle(measure, on)"
        @select.prevent
      >
        {{ format.measureLabel(measure) }}
      </DropdownMenuCheckboxItem>
      <template v-if="numeric.length > 0">
        <DropdownMenuSeparator />
        <DropdownMenuSub
          v-for="field in numeric"
          :key="field.key"
        >
          <DropdownMenuSubTrigger>{{ format.fieldLabel(field) }}</DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuCheckboxItem
              v-for="stat in stats"
              :key="stat"
              :model-value="chosen({ stat, field })"
              :disabled="!chosen({ stat, field }) && full"
              @update:model-value="(on) => toggle({ stat, field }, on)"
              @select.prevent
            >
              {{ format.measureLabel({ stat, field }) }}
            </DropdownMenuCheckboxItem>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      </template>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
