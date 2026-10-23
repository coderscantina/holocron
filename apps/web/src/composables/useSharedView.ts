import { useQuery } from '@tanstack/vue-query'
import { computed, ref, shallowRef, watch } from 'vue'

import { ApiError } from '~/api'
import { analyticsCursor, analyticsTopic } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import { sharedViewResource, shareClient, type ShareScope } from '~/api/resources/sharing'
import type { ExplorationState } from '~/generated'
import { queryClient } from '~/plugins/vue-query'
import { normalize } from '~/services/exploration'

import { useExplore } from './useExplore'
import { useSubscription } from './useLive'

/** Drops everything read through a token, after the share was revoked or expired. */
export function forgetShare(token: string): void {
  void queryClient.cancelQueries({ queryKey: queryKeys.share(token) })
  queryClient.removeQueries({ queryKey: queryKeys.share(token) })
}

const isNotFound = (error: unknown) => error instanceof ApiError && error.code === 'not_found'

/**
 * A share as its viewer sees it: the token's own client, never the person's session, and the
 * resolved share. A revoked or expired share answers `not_found` everywhere; once it does, the
 * page stops reading through it and drops what it showed. One token per component instance:
 * key the caller by token.
 */
export function useShare(token: string) {
  const scope: ShareScope = { token, client: shareClient(token) }
  /** Set for good once the share answers `not_found`. */
  const gone = ref(false)

  const view = useQuery({
    queryKey: queryKeys.shareView(token),
    queryFn: ({ signal }) => sharedViewResource.resolve(scope.client, signal),
    staleTime: Infinity,
    enabled: computed(() => !gone.value),
  })

  /** Ends the page's reads when `error` says the share itself is gone. */
  const check = (error: unknown) => {
    if (gone.value || !isNotFound(error)) return
    gone.value = true
    forgetShare(token)
  }
  watch(() => view.error.value, check)

  return { scope, view, gone, check }
}

export type Share = ReturnType<typeof useShare>

/**
 * The exploration of a share. The viewer's state starts as the shared view and changes locally
 * (paging, sorting, drilldowns, range); the server refuses anything outside the share's limits
 * with `share_scope_exceeded`. Newer data arrives through the site's analytics topic, read with
 * the share's credential.
 */
export function useSharedExploration(share: Share) {
  const shared = computed(() => {
    const target = share.view.data.value?.target
    return target?.kind === 'exploration' ? target : null
  })
  const state = shallowRef<ExplorationState | null>(null)
  watch(
    shared,
    (target) => {
      state.value = target ? normalize(target.state) : null
    },
    { immediate: true }
  )

  const explore = useExplore(() => (share.gone.value ? null : state.value), [], share.scope)
  watch(() => explore.query.error.value, share.check)

  const exceeded = computed(
    () => explore.query.error.value instanceof ApiError && explore.query.error.value.code === 'share_scope_exceeded'
  )

  const site = computed(() => state.value?.site ?? null)
  const subscription = useSubscription(
    () => (site.value && !share.gone.value ? analyticsTopic(site.value) : null),
    () => {
      const batch = explore.query.data.value?.freshness.watermark_batch
      return site.value && batch !== undefined ? analyticsCursor(site.value, batch) : null
    },
    {
      events: explore.refresh,
      // The share may be gone: resolving it again either succeeds or answers `not_found`.
      reset: () => {
        if (!share.gone.value) void share.view.refetch()
      },
    },
    share.scope.client
  )

  return {
    shared,
    state,
    result: explore.query,
    exceeded,
    live: subscription.connected,
    refresh: explore.refresh,
    commit: (next: ExplorationState) => {
      state.value = normalize(next)
    },
    reset: () => {
      state.value = shared.value ? normalize(shared.value.state) : null
    },
  }
}
