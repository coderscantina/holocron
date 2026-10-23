<script setup lang="ts">
import { useRoute, useRouter } from 'vue-router'

import { openFinding } from '~/components/findings/actions'
import { useFindingText } from '~/components/findings/useFindingText'
import CommandResultGroup from '~/components/CommandResultGroup.vue'
import { useCommandPalette } from '~/composables/useCommandPalette'
import { useCurrentFindings } from '~/composables/useCurrentFindings'
import { useGoals } from '~/composables/useGoals'
import { useSavedExplorations, useSegments } from '~/composables/useDefinitions'
import { useSite } from '~/composables/useSiteContext'
import type { ExplorationState } from '~/generated'
import { pageOf, type StatePage } from '~/lib/state-pages'
import { useI18n } from '~/plugins/i18n'
import { applySegment, decodeState, defaultState, encodeState, fitsUrl, normalize } from '~/services/exploration'
import { goalsState } from '~/services/reports'

/**
 * Saved views, segments and goals of one site, read through the caches their own screens share.
 * Only mounted while the palette is open, so a cached list costs no request.
 */
const props = defineProps<{ siteId: string }>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const palette = useCommandPalette()
const views = useSavedExplorations(() => props.siteId)
const segments = useSegments(() => props.siteId)
const goals = useGoals(() => props.siteId)
const site = useSite(() => props.siteId)
const siteFindings = useCurrentFindings()
const words = useFindingText(() => ({ id: props.siteId, timezone: site.data.value?.site.timezone ?? 'UTC' }))

const viewItems = computed(() => (views.list.data.value?.explorations ?? []).map((view) => ({ ...view, label: view.name })))
const segmentItems = computed(() => (segments.list.data.value?.segments ?? []).map((segment) => ({ ...segment, label: segment.name })))
const goalItems = computed(() => goals.goals.value.map((goal) => ({ ...goal, label: goal.name })))

const findingItems = computed(() =>
  siteFindings.stale.value
    ? []
    : siteFindings.findings.value.map((finding) => {
        const text = words.text(finding)
        return { id: String(finding.rank), label: text.fact, finding, known: text.known }
      })
)

const stateRoute = (name: StatePage, state: ExplorationState) =>
  ({ name, params: { siteId: props.siteId }, query: { s: encodeState(state) } }) as const

const timezone = async () => site.data.value?.site.timezone ?? (await site.refetch()).data?.site.timezone

/** The state on screen when this site's exploration page shows one, else Explore's default. */
const currentState = async (): Promise<{ page: StatePage; state: ExplorationState } | null> => {
  const text = route.query.s
  const decoded = typeof text === 'string' ? decodeState(text) : null
  if (decoded?.ok && decoded.state.site === props.siteId) return { page: pageOf(decoded.state), state: decoded.state }
  const zone = await timezone()
  return zone ? { page: 'explore', state: defaultState(props.siteId, zone) } : null
}

const go = (page: StatePage, state: ExplorationState) => {
  palette.close()
  return router.push(stateRoute(page, state))
}

const openFindingItem = (item: (typeof findingItems.value)[number]) => {
  palette.close()
  return router.push(openFinding(props.siteId, item.finding, item.known))
}

const addSegment = async (filter: Parameters<typeof applySegment>[1]) => {
  const current = await currentState()
  if (!current) return
  const next = normalize(applySegment(current.state, filter))
  if (fitsUrl(next)) await go(current.page, next)
}

const openGoal = async (goal: (typeof goalItems.value)[number]) => {
  const zone = await timezone()
  if (zone) await go('goals', goalsState(props.siteId, zone, [goal]))
}
</script>

<template>
  <CommandResultGroup
    :heading="t('commandPalette.findings')"
    icon="lucide:sparkles"
    :items="findingItems"
    @select="openFindingItem"
  />
  <CommandResultGroup
    :heading="t('commandPalette.savedViews')"
    icon="lucide:bookmark"
    :items="viewItems"
    @select="(view) => go(pageOf(view.state), view.state)"
  />
  <CommandResultGroup
    :heading="t('commandPalette.segments')"
    icon="lucide:users-round"
    :items="segmentItems"
    @select="(segment) => addSegment(segment.filter)"
  />
  <CommandResultGroup
    :heading="t('commandPalette.goals')"
    icon="lucide:target"
    :items="goalItems"
    @select="openGoal"
  />
</template>
