<script setup lang="ts">
import FilterBar from '~/components/exploration/FilterBar.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeaderCombined } from '~/components/ui/dialog'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFilterFields } from '~/composables/useFilterFields'
import { useGoals } from '~/composables/useGoals'
import type { ExplorationState, GoalCounting, PopulationScope, SavedGoal, Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { conditions } from '~/services/exploration'
import { goalsOf, goalsState } from '~/services/reports'

import ChoiceGroup from './ChoiceGroup.vue'

/**
 * Creates a goal, or saves the next version of one. The predicate is edited with the filter bar
 * on a draft goals state; saving stores a new version and earlier ones stay as they were, so
 * reports that copied them keep their meaning (0003 Goals).
 */
const props = defineProps<{
  site: Site
  /** The goal whose next version this saves; a new goal without it. */
  goal?: SavedGoal | null
}>()

const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ saved: [goal: SavedGoal] }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { fields } = useFilterFields(() => props.site.id)
const goals = useGoals(() => props.site.id)

const name = ref('')
const scope = ref<PopulationScope>('session')
const counting = ref<GoalCounting>('once')
const draft = ref<ExplorationState | null>(null)
const error = ref<string | null>(null)

watch(
  open,
  (value) => {
    if (!value) return
    const base = goalsState(props.site.id, props.site.timezone, [])
    const definition = props.goal?.definition ?? {
      predicate: { kind: 'group', op: 'and', items: [] },
      scope: 'session',
      counting: 'once',
    }
    draft.value = { ...base, report: { kind: 'goals', v: 1, goals: [definition] } }
    name.value = props.goal?.name ?? ''
    scope.value = definition.scope
    counting.value = definition.counting
    error.value = null
  },
  { immediate: true }
)

const predicate = computed(() => (draft.value ? goalsOf(draft.value)[0]?.predicate : undefined))
const ready = computed(() => name.value.trim() !== '' && !!predicate.value && conditions(predicate.value).length > 0)

const save = async () => {
  const tree = predicate.value
  if (!ready.value || !tree) return
  error.value = null
  try {
    const saved = await goals.save.mutateAsync({
      name: name.value,
      definition: { predicate: tree, scope: scope.value, counting: counting.value },
      goal: props.goal?.id,
    })
    open.value = false
    emit('saved', saved.goal)
  } catch (failure) {
    error.value = message(failure)
  }
}
const nameId = useId()
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-xl">
      <DialogHeaderCombined
        :title="goal ? t('reports.goals.editTitle', { name: goal.name }) : t('reports.goals.newTitle')"
        :description="goal ? t('reports.goals.editDescription', { version: goal.version + 1 }) : t('reports.goals.newDescription')"
      />
      <form
        class="grid gap-4"
        @submit.prevent="save"
      >
        <div class="grid gap-1.5">
          <Label :for="nameId">{{ t('reports.goals.name') }}</Label>
          <Input
            :id="nameId"
            v-model="name"
            maxlength="100"
            required
          />
        </div>
        <div class="grid gap-1.5">
          <span class="text-sm font-semibold text-foreground">{{ t('reports.goals.predicate') }}</span>
          <FilterBar
            v-if="draft"
            :state="draft"
            :fields="fields"
            :tree="{ kind: 'goal', index: 0 }"
            :label="t('reports.goals.predicate')"
            @commit="(next) => (draft = next)"
          />
        </div>
        <div class="flex flex-wrap gap-x-6 gap-y-3">
          <div class="grid gap-1.5">
            <span class="text-xs font-semibold text-muted">{{ t('reports.goals.scope') }}</span>
            <ChoiceGroup
              v-model="scope"
              :label="t('reports.goals.scope')"
              :options="[
                { value: 'session', label: t('reports.scope.session') },
                { value: 'person', label: t('reports.scope.person') },
              ]"
            />
          </div>
          <div class="grid gap-1.5">
            <span class="text-xs font-semibold text-muted">{{ t('reports.goals.counting') }}</span>
            <ChoiceGroup
              v-model="counting"
              :label="t('reports.goals.counting')"
              :options="[
                { value: 'once', label: t('reports.goals.once') },
                { value: 'every', label: t('reports.goals.every') },
              ]"
            />
          </div>
        </div>
        <p
          v-if="error"
          class="text-sm text-destructive"
          role="alert"
        >
          {{ error }}
        </p>
        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            @click="open = false"
          >
            {{ t('explore.views.cancel') }}
          </Button>
          <Button
            type="submit"
            variant="accent"
            :disabled="!ready || goals.save.isPending.value"
          >
            {{ goal ? t('reports.goals.saveVersion') : t('reports.goals.create') }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
