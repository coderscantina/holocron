import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { computed, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import { shareKey, type ShareScope } from '~/api/resources/sharing'
import type { ExplorationState, Panel } from '~/generated'
import { queryClient } from '~/plugins/vue-query'
import { canonical } from '~/services/exploration'

/**
 * Drops every cached result of a site. Called when the server refuses the site, so nothing of
 * a revoked scope stays on screen or in memory. The site itself is read again, so its boundary
 * replaces the page with the "not available" state instead of keeping old figures.
 */
export function forgetSite(site: string): void {
  const ofSite = (key: readonly unknown[]) => key[1] === site
  void queryClient.cancelQueries({ predicate: (query) => ofSite(query.queryKey) })
  queryClient.removeQueries({ predicate: (query) => ofSite(query.queryKey) && query.queryKey[0] !== 'sites.get' })
  void queryClient.resetQueries({ queryKey: queryKeys.site(site) })
  void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
}

export const isRefusal = (error: unknown) =>
  error instanceof ApiError && (error.code === 'not_found' || error.code === 'forbidden')

/** Forgets the site's results once a query of it is refused. */
export function watchScope(site: MaybeRefOrGetter<string | null | undefined>, error: () => unknown) {
  watch(error, (value) => {
    const id = toValue(site)
    if (id && isRefusal(value)) forgetSite(id)
  })
}

/**
 * Server results of one state and its panels. Keyed by the canonical state and a refresh
 * epoch, never refetched on focus or on collected events: historical reports change only on a
 * deliberate refresh. A newer state cancels the older request; the previous result stays
 * visible, labelled, only while the site is the same.
 */
export function useExplore(
  state: MaybeRefOrGetter<ExplorationState | null>,
  panels: MaybeRefOrGetter<Panel[]> = [],
  /** Reads as a share viewer instead of the person. */
  share?: ShareScope
) {
  const epoch = ref(0)
  const key = computed(() => {
    const current = toValue(state)
    return shareKey(
      share,
      current
        ? queryKeys.explore(current.site, canonical(current), canonical(toValue(panels)), epoch.value)
        : (['analytics.explore', null] as const)
    )
  })
  const query = useQuery({
    queryKey: key,
    queryFn: ({ signal }) => {
      const current = toValue(state)
      if (!current) throw new Error('no state')
      return explorationsResource.explore(current, toValue(panels), signal, share?.client)
    },
    enabled: computed(() => toValue(state) !== null),
    staleTime: Infinity,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[share ? 3 : 1] === toValue(state)?.site ? keepPreviousData(previous) : undefined,
  })
  // A share's refusals belong to the share page, not to the person's own site cache.
  if (!share) watchScope(() => toValue(state)?.site, () => query.error.value)
  return {
    query,
    /** The shown result belongs to an earlier state while the new one loads. */
    previous: computed(() => query.isPlaceholderData.value),
    refresh: () => {
      epoch.value += 1
    },
  }
}
