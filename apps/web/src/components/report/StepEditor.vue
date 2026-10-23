<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import FilterBar from '~/components/exploration/FilterBar.vue'
import { Button } from '~/components/ui/button'
import type { FilterField } from '~/composables/useFilterFields'
import type { ExplorationState } from '~/generated'
import { REPORT_FUNNEL_STEPS_MAX, REPORT_FUNNEL_STEPS_MIN } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'
import { conditions } from '~/services/exploration'
import { addStep, funnelOf, moveStep, removeStep } from '~/services/reports'

import PredicateQuickAdd from './PredicateQuickAdd.vue'

/**
 * The ordered steps of a funnel as a numbered list. Each step is a predicate tree edited with
 * the filter bar; buttons move a step up or down and remove it, so the order changes without
 * dragging. Every change is one new state.
 */
const props = defineProps<{
  state: ExplorationState
  fields: FilterField[]
  /** Server refusals by predicate ID. */
  refused: Record<string, string>
}>()

const emit = defineEmits<{ commit: [state: ExplorationState] }>()

const { t } = useI18n()
const steps = computed(() => funnelOf(props.state)?.steps ?? [])
const announcement = ref('')

const move = (index: number, delta: -1 | 1) => {
  emit('commit', moveStep(props.state, index, delta))
  announcement.value = t('reports.funnel.moved', { from: index + 1, to: index + 1 + delta })
}
const remove = (index: number) => {
  emit('commit', removeStep(props.state, index))
  announcement.value = t('reports.funnel.removed', { step: index + 1 })
}
</script>

<template>
  <div class="grid gap-3">
    <ol
      class="grid gap-2"
      :aria-label="t('reports.funnel.steps')"
    >
      <li
        v-for="(step, index) in steps"
        :key="index"
        class="flex items-start gap-2 rounded-panel border border-hairline bg-card p-2 max-sm:flex-wrap"
      >
        <span
          class="mt-1.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-fill-strong text-xs font-semibold text-primary tabular-nums"
          aria-hidden="true"
        >
          {{ index + 1 }}
        </span>
        <div class="min-w-0 flex-1 max-sm:basis-[calc(100%-2rem)]">
          <FilterBar
            :state="state"
            :fields="fields"
            :refused="refused"
            :tree="{ kind: 'step', index }"
            :label="t('reports.funnel.stepLabel', { step: index + 1 })"
            @commit="(next) => emit('commit', next)"
          />
          <p
            v-if="conditions(step).length === 0"
            class="mt-1 text-xs text-warning"
            role="status"
          >
            {{ t('reports.funnel.emptyStep') }}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-1 max-sm:ml-8">
          <Button
            variant="ghost"
            size="toolbar"
            :disabled="index === 0"
            :aria-label="t('reports.funnel.moveUp', { step: index + 1 })"
            @click="move(index, -1)"
          >
            <Icon name="lucide:arrow-up" />
          </Button>
          <Button
            variant="ghost"
            size="toolbar"
            :disabled="index === steps.length - 1"
            :aria-label="t('reports.funnel.moveDown', { step: index + 1 })"
            @click="move(index, 1)"
          >
            <Icon name="lucide:arrow-down" />
          </Button>
          <Button
            variant="ghost"
            size="toolbar"
            :disabled="steps.length <= REPORT_FUNNEL_STEPS_MIN"
            :aria-label="t('reports.funnel.remove', { step: index + 1 })"
            @click="remove(index)"
          >
            <Icon name="lucide:trash-2" />
          </Button>
        </div>
      </li>
    </ol>
    <PredicateQuickAdd
      :action="t('reports.funnel.add')"
      :disabled="steps.length >= REPORT_FUNNEL_STEPS_MAX"
      @add="(items) => emit('commit', addStep(state, items))"
    />
    <span
      class="sr-only"
      role="status"
      aria-live="polite"
    >
      {{ announcement }}
    </span>
  </div>
</template>
