import { useQuery } from '@tanstack/vue-query'
import { computed, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { analyticsCursor, analyticsTopic } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import { summariesResource } from '~/api/resources/summaries'
import type { SiteId } from '~/generated'

import { watchScope } from './useExplore'
import { useSubscription } from './useLive'

const MINUTE_MS = 60_000

/**
 * A site's findings from `summaries.findings` (one SQLite read, never an analytical query), at
 * most `limit` of them in rank order. The `analytics:<site>` stream refreshes them on every
 * `summary` event and after a reconnect; there is no polling.
 *
 * A null site reads nothing. `findings` is empty until the first answer. `stale` is true without a summary, as the server
 * reports it, and `computedAt` is null before the first summary.
 */
export function useSiteFindings(site: MaybeRefOrGetter<SiteId | null>, limit: MaybeRefOrGetter<number>) {
  const query = useQuery({
    queryKey: computed(() => queryKeys.findings(toValue(site) ?? '', toValue(limit))),
    queryFn: ({ signal }) => summariesResource.findings(toValue(site) ?? '', toValue(limit), signal),
    enabled: computed(() => toValue(site) !== null),
    staleTime: MINUTE_MS,
  })
  watchScope(site, () => query.error.value)

  /** The stream starts at the batch of the first answer for a site; later answers must not restart it. */
  const start = shallowRef<{ site: SiteId; cursor: string } | null>(null)
  const seed = () => {
    const answer = query.data.value
    if (answer) start.value = { site: answer.site, cursor: analyticsCursor(answer.site, answer.watermark_batch ?? 0) }
  }
  watch(
    () => query.data.value?.site,
    (answered) => {
      if (answered && start.value?.site !== answered) seed()
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
        seed()
      },
    }
  )
  let connectedBefore = false
  watch(subscription.connected, (connected) => {
    if (!connected) return
    if (connectedBefore) void query.refetch()
    connectedBefore = true
  })

  return {
    query,
    findings: computed(() => query.data.value?.findings ?? []),
    stale: computed(() => query.data.value?.stale ?? false),
    computedAt: computed(() => query.data.value?.computed_at ?? null),
  }
}
