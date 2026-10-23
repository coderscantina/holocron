import { useMutation, useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import type {
  RecalculationId,
  RecalculationsStartArgs,
  SitesUpdateIdentityPolicyArgs,
  SitesUpdateSessionPolicyArgs,
} from '~/generated'
import { queryClient } from '~/plugins/vue-query'

/** Policy and recalculation commands change the site's revision, so the site snapshot is read again. */
const refreshSite = (site: string) => queryClient.invalidateQueries({ queryKey: queryKeys.site(site) })

export function useUpdateSessionPolicy() {
  return useMutation({
    mutationFn: ({ args, idempotencyKey }: { args: SitesUpdateSessionPolicyArgs; idempotencyKey: string }) =>
      sitesResource.updateSessionPolicy(args, idempotencyKey),
    onSuccess: (answer) => refreshSite(answer.site.id),
  })
}

/** Readable with `site.configure` only; pass `enabled: false` for other roles. */
export function useIdentityPolicy(site: MaybeRefOrGetter<string>, enabled: MaybeRefOrGetter<boolean>) {
  return useQuery({
    queryKey: computed(() => queryKeys.identityPolicy(toValue(site))),
    queryFn: ({ signal }) => sitesResource.identityPolicy(toValue(site), signal),
    enabled: () => toValue(enabled),
  })
}

export function useUpdateIdentityPolicy() {
  return useMutation({
    mutationFn: ({ args, idempotencyKey }: { args: SitesUpdateIdentityPolicyArgs; idempotencyKey: string }) =>
      sitesResource.updateIdentityPolicy(args, idempotencyKey),
    onSuccess: (_, { args }) =>
      Promise.all([
        refreshSite(args.site),
        queryClient.invalidateQueries({ queryKey: queryKeys.identityPolicy(args.site) }),
      ]),
  })
}

export function useStartRecalculation() {
  return useMutation({
    mutationFn: ({ args, idempotencyKey }: { args: RecalculationsStartArgs; idempotencyKey: string }) =>
      sitesResource.recalculate(args, idempotencyKey),
    onSuccess: (_, { args }) => refreshSite(args.site),
  })
}

/** Polls a recalculation every two seconds until correction has caught up with its range. */
export function useRecalculation(site: MaybeRefOrGetter<string>, recalculation: MaybeRefOrGetter<RecalculationId | null>) {
  return useQuery({
    queryKey: computed(() => queryKeys.recalculation(toValue(site), toValue(recalculation) ?? '')),
    queryFn: ({ signal }) => {
      const id = toValue(recalculation)
      if (!id) throw new Error('no recalculation to read')
      return sitesResource.recalculation(toValue(site), id, signal)
    },
    enabled: () => toValue(recalculation) !== null,
    refetchInterval: (query) => (query.state.data?.recalculation.state === 'pending' ? 2000 : false),
  })
}
