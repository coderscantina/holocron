import { useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { recordingsResource } from '~/api/resources/recordings'
import { lastDays } from '~/composables/useRecordings'
import type { Group, Site } from '~/generated'
import { canonical } from '~/services/exploration'

const MINUTE_MS = 60_000

/**
 * How many replays of the last day match a filter, for the "Watch {n} replays" action; null
 * while the filter is absent, the person cannot read recordings, replay recording is off for
 * the site, or the count has not arrived. The range is the replay page's own "1d", so the link
 * lists exactly the counted replays.
 */
export function useFindingReplays(site: MaybeRefOrGetter<Site>, filter: MaybeRefOrGetter<Group | null>) {
  const allowed = computed(() => toValue(site).capabilities.includes('site.recordings.read'))
  const query = useQuery({
    queryKey: computed(() => ['findings.replays', toValue(site).id, canonical(toValue(filter))] as const),
    queryFn: async ({ signal }) => {
      const { id } = toValue(site)
      const group = toValue(filter)
      const { policy } = await recordingsResource.policy(id, signal)
      if (!policy.replay_enabled || !group) return null
      const page = await recordingsResource.list({ site: id, ...lastDays(1), filter: canonical(group), limit: 1 }, signal)
      return page.total ?? null
    },
    enabled: computed(() => allowed.value && toValue(filter) !== null),
    staleTime: MINUTE_MS,
  })
  return computed(() => query.data.value ?? null)
}
