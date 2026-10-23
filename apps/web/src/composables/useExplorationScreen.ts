import { computed, ref, toValue, type MaybeRefOrGetter } from 'vue'

import { ApiError } from '~/api'
import type { ExplorationState, Panel } from '~/generated'

import { useErrorMessage } from './useErrorMessage'
import { useExplore } from './useExplore'
import { useExploration } from './useExploration'

export interface Notice {
  message: string
  tone: 'success' | 'error'
}

/**
 * One exploration screen: the URL state, its results, and what the screen says about them:
 * a notice after an action, the server's refusal of the state, and refusals per condition.
 */
export function useExplorationScreen(
  site: MaybeRefOrGetter<string>,
  defaults: () => ExplorationState | null,
  /** Panels computed with the state, derived from it. */
  panels: (state: ExplorationState | null) => Panel[] = () => [],
  /** Whether a state is complete enough to send; a report builder holds back half-built steps. */
  ready: (state: ExplorationState) => boolean = () => true
) {
  const exploration = useExploration(site, defaults)
  const sent = computed(() => {
    const state = exploration.state.value
    return state && ready(state) ? state : null
  })
  const { query, previous, refresh } = useExplore(sent, () => panels(exploration.state.value))
  const notice = ref<Notice | null>(null)
  const { message, fieldMessage } = useErrorMessage()

  /** A refusal of the state itself; scope refusals are the site boundary's to show. */
  const refusal = computed(() => {
    const error = query.error.value
    return error instanceof ApiError && error.code !== 'not_found' && error.code !== 'forbidden' ? error : null
  })

  /**
   * Translated errors of conditions by predicate ID, so the refused chip can say why. IDs are
   * unique across the state, so `filter.<id>` and report trees like `report.steps.2.<id>` map
   * to the same chips.
   */
  const refused = computed<Record<string, string>>(() => {
    const error = refusal.value
    if (!error) return {}
    return Object.fromEntries(
      Object.keys(error.body.errors ?? {}).flatMap((field) => {
        const id = /^(?:filter|report\.[a-z0-9_.]+)\.([a-z0-9_]{1,16})$/.exec(field)?.[1]
        return id ? [[id, `${message(error)} ${fieldMessage(error, field) ?? ''}`.trim()]] : []
      })
    )
  })

  const commit = (next: ExplorationState) => {
    notice.value = null
    return exploration.commit(next)
  }

  return {
    ...exploration,
    commit,
    query,
    result: computed(() => query.data.value ?? null),
    previous,
    refresh,
    notice,
    refusal,
    refused,
    siteId: computed(() => toValue(site)),
  }
}
