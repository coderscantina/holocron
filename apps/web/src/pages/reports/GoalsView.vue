<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import GoalEditor from '~/components/report/GoalEditor.vue'
import PopulationNote from '~/components/report/PopulationNote.vue'
import ReportBars from '~/components/report/ReportBars.vue'
import ReportScreen from '~/components/report/ReportScreen.vue'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useGoals } from '~/composables/useGoals'
import { usePredicateText } from '~/composables/usePredicateText'
import type { ExplorationState, GoalDefinition, ReportResult, SavedGoal, Site } from '~/generated'
import { REPORT_GOALS_MAX } from '~/generated/limits'
import { pageOf } from '~/lib/state-pages'
import { useI18n } from '~/plugins/i18n'
import { encodeState } from '~/services/exploration'
import {
  addGoal,
  complete,
  eventsFor,
  goalsOf,
  goalsState,
  removeGoal,
  replaceGoal,
  sessionsFor,
  upgradeFor,
} from '~/services/reports'

/**
 * Conversion of the site's goals (0003 Goals). The report holds each goal inline with the
 * saved version it came from; a newer saved version is offered as an upgrade, never applied
 * silently. Editing a goal saves its next version.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const format = useExplorationFormat()
const { text } = usePredicateText()
const goals = useGoals(() => props.site.id)
const saved = goals.goals

const screen = useExplorationScreen(
  () => props.site.id,
  () => (goals.list.isSuccess.value ? goalsState(props.site.id, props.site.timezone, saved.value) : null),
  () => [],
  complete
)
const canWrite = computed(() => props.site.capabilities.includes('site.definitions.write'))
const inReport = computed(() => new Set(goalsOf(screen.state.value ?? goalsState('', 'UTC', [])).map((g) => g.ref?.id)))
const addable = computed(() => saved.value.filter((goal) => !inReport.value.has(goal.id)))

const editorOpen = ref(false)
const editing = ref<SavedGoal | null>(null)
const startEditing = (goal: SavedGoal | null) => {
  editing.value = goal
  editorOpen.value = true
}
/** A new goal joins the report; a new version of a goal in it replaces that goal. */
const onSaved = (goal: SavedGoal) => {
  const state = screen.state.value
  if (!state) return
  const index = goalsOf(state).findIndex((g) => g.ref?.id === goal.id)
  void screen.commit(index >= 0 ? replaceGoal(state, index, goal) : addGoal(state, goal))
}

const savedOf = (goal: GoalDefinition) => saved.value.find((s) => s.id === goal.ref?.id) ?? null
const nameOf = (goal: GoalDefinition) => {
  const own = savedOf(goal)
  return own ? own.name : text(goal.predicate)
}
const link = (state: ExplorationState) => ({
  name: pageOf(state),
  params: { siteId: state.site },
  query: { s: encodeState(state) },
})

/** One population note per scope in the report: session goals share one, person goals another. */
const notes = (state: ExplorationState, report: ReportResult) => {
  if (report.kind !== 'goals') return []
  const definitions = goalsOf(state)
  return (['session', 'person'] as const).flatMap((scope) => {
    const index = definitions.findIndex((goal) => goal.scope === scope)
    const coverage = report.goals[index]?.coverage
    return coverage ? [{ scope, coverage }] : []
  })
}

const rows = (state: ExplorationState, report: ReportResult) => {
  if (report.kind !== 'goals') return []
  const definitions = goalsOf(state)
  const rates = report.goals.map((g) => (g.rate.t === 'ratio' ? g.rate.v : 0))
  const top = Math.max(1, ...rates)
  return report.goals.flatMap((result, index) => {
    const goal = definitions[index]
    if (!goal) return []
    const own = savedOf(goal)
    return [
      {
        key: String(index),
        index,
        goal,
        own,
        upgrade: upgradeFor(goal, saved.value),
        label: nameOf(goal),
        share: (rates[index] ?? 0) / top,
        values: [
          format.metric(result.rate),
          format.metric({ t: 'count', v: result.completions }),
          format.metric({ t: 'count', v: result.denominator }),
          t(goal.scope === 'person' ? 'reports.goals.personDenominator' : 'reports.goals.sessionDenominator'),
          t(goal.counting === 'once' ? 'reports.goals.once' : 'reports.goals.every'),
          goal.ref ? t('reports.goals.version', { version: goal.ref.version }) : t('reports.goals.unsaved'),
        ],
      },
    ]
  })
}
</script>

<template>
  <ReportScreen
    :site="site"
    :screen="screen"
    :title="t('reports.goals.title')"
  >
    <template #builder="{ state }">
      <div class="flex flex-wrap items-center gap-2">
        <DropdownMenu>
          <DropdownMenuTrigger as-child>
            <Button
              variant="fill"
              size="sm"
              :disabled="addable.length === 0 || goalsOf(state).length >= REPORT_GOALS_MAX"
            >
              <Icon
                name="lucide:list-plus"
                aria-hidden="true"
              />
              {{ t('reports.goals.add') }}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="start"
            class="w-72"
          >
            <DropdownMenuItem
              v-for="goal in addable"
              :key="goal.id"
              @select="screen.commit(addGoal(state, goal))"
            >
              <span class="truncate">{{ goal.name }}</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          v-if="canWrite"
          variant="accent"
          size="sm"
          @click="startEditing(null)"
        >
          <Icon
            name="lucide:plus"
            aria-hidden="true"
          />
          {{ t('reports.goals.new') }}
        </Button>
        <p
          v-if="saved.length === 0 && goals.list.isSuccess.value"
          class="text-sm text-muted"
        >
          {{ canWrite ? t('reports.goals.noneWriter') : t('reports.goals.noneReader') }}
        </p>
      </div>
    </template>
    <template #result="{ state, report }">
      <Panel
        v-if="report.kind === 'goals'"
        :title="t('reports.goals.table')"
        flush
      >
        <template #actions>
          <PopulationNote :notes="notes(state, report)" />
        </template>
        <ReportBars
          :caption="t('reports.goals.table')"
          :columns="[
            t('reports.goals.goal'),
            t('reports.goals.rate'),
            t('reports.goals.completions'),
            t('reports.goals.denominator'),
            t('reports.goals.of'),
            t('reports.goals.counting'),
            t('reports.goals.versionColumn'),
          ]"
          :rows="rows(state, report)"
        >
          <template #actions="{ row }">
            <DropdownMenu>
              <DropdownMenuTrigger as-child>
                <Button
                  variant="ghost"
                  size="toolbar"
                  class="text-muted"
                  :aria-label="t('reports.drill.actionsFor', { value: row.label })"
                >
                  <Icon
                    name="lucide:ellipsis"
                    aria-hidden="true"
                  />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuLabel class="max-w-64 truncate text-xs text-muted">{{ row.label }}</DropdownMenuLabel>
                <DropdownMenuItem
                  v-if="row.upgrade"
                  @select="screen.commit(replaceGoal(state, row.index, row.upgrade))"
                >
                  <Icon
                    name="lucide:circle-arrow-up"
                    aria-hidden="true"
                  />
                  {{ t('reports.goals.upgrade', { version: row.upgrade.version }) }}
                </DropdownMenuItem>
                <DropdownMenuItem
                  v-if="canWrite && row.own"
                  @select="startEditing(row.own)"
                >
                  <Icon
                    name="lucide:pencil"
                    aria-hidden="true"
                  />
                  {{ t('reports.goals.edit') }}
                </DropdownMenuItem>
                <DropdownMenuItem
                  :disabled="goalsOf(state).length <= 1"
                  @select="screen.commit(removeGoal(state, row.index))"
                >
                  <Icon
                    name="lucide:x"
                    aria-hidden="true"
                  />
                  {{ t('reports.goals.remove') }}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem as-child>
                  <RouterLink :to="link(sessionsFor(state, [row.goal.predicate]))">
                    <Icon
                      name="lucide:users"
                      aria-hidden="true"
                    />
                    {{ t('reports.drill.sessions') }}
                  </RouterLink>
                </DropdownMenuItem>
                <DropdownMenuItem as-child>
                  <RouterLink :to="link(eventsFor(state, [row.goal.predicate]))">
                    <Icon
                      name="lucide:search"
                      aria-hidden="true"
                    />
                    {{ t('reports.drill.events') }}
                  </RouterLink>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </template>
        </ReportBars>
      </Panel>
    </template>
  </ReportScreen>
  <GoalEditor
    v-model:open="editorOpen"
    :site="site"
    :goal="editing"
    @saved="onSaved"
  />
</template>
