import { useQuery } from '@tanstack/vue-query'
import { computed, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { analyticsCursor, analyticsTopic } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import { summariesResource } from '~/api/resources/summaries'
import type { SiteId, Summary, WorkspaceId } from '~/generated'

import { useSubscription } from './useLive'

const MINUTE_MS = 60_000

/**
 * Site summaries from `summaries.forSites` (one small SQLite read per call, never an analytical
 * query), keyed by workspace and the sites in view.
 *
 * `follow` names the one site whose `analytics:<site>` stream is open; a `summary` event on it,
 * and every reconnect after the first connect (events are not replayed), read the summaries
 * again. One stream per site would exhaust the browser's connections, so other sites refresh
 * with `refreshIfStale`, called when a menu opens. There is no polling.
 */
export function useSiteSummaries(
  workspace: MaybeRefOrGetter<WorkspaceId | null>,
  siteIds: MaybeRefOrGetter<readonly SiteId[]>,
  follow: MaybeRefOrGetter<SiteId | null> = null
) {
  const sorted = computed(() => [...toValue(siteIds)].sort())
  const query = useQuery({
    queryKey: computed(() => queryKeys.summaries(toValue(workspace) ?? '', sorted.value)),
    queryFn: ({ signal }) => summariesResource.forSites(sorted.value, signal),
    enabled: computed(() => toValue(workspace) !== null && sorted.value.length > 0),
    staleTime: MINUTE_MS,
  })

  const summaries = computed(() => new Map<SiteId, Summary>((query.data.value ?? []).map((entry) => [entry.site, entry])))

  /** The cursor the stream starts from, taken once per followed site: later summaries must not restart it. */
  const start = shallowRef<{ site: SiteId; cursor: string } | null>(null)
  const seed = (site: SiteId | null) => {
    start.value = site
      ? { site, cursor: analyticsCursor(site, summaries.value.get(site)?.watermark_batch ?? 0) }
      : null
  }
  watch(
    [() => toValue(follow), query.isSuccess],
    ([site, loaded]) => {
      if (!site) seed(null)
      else if (loaded && start.value?.site !== site) seed(site)
    },
    { immediate: true }
  )

  const subscription = useSubscription(
    () => start.value && analyticsTopic(start.value.site),
    () => start.value?.cursor ?? null,
    {
      events: (events) => {
        if (events.some((event) => event.kind === 'summary')) void query.refetch()
      },
      reset: async () => {
        await query.refetch()
        seed(toValue(follow))
      },
    }
  )

  let connectedBefore = false
  watch(() => start.value?.site, () => {
    connectedBefore = false
  })
  watch(subscription.connected, (connected) => {
    if (!connected) return
    if (connectedBefore) void query.refetch()
    connectedBefore = true
  })

  return {
    query,
    summaries,
    /** Reads again only when the cached answer is older than a minute. */
    refreshIfStale: () => {
      if (query.isStale.value) void query.refetch()
    },
  }
}
