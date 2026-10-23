import { useQuery } from '@tanstack/vue-query'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import { closeAllLive } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import { sessionResource } from '~/api/resources/session'
import type { LoginResult, SessionState } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

const signedOut: SessionState = { account: null, enrollment_only: false }

/** Loads the session once; the router guard awaits the same query before the first page. */
export const loadSession = () =>
  queryClient.ensureQueryData({
    queryKey: queryKeys.session,
    queryFn: ({ signal }) => sessionResource.current(signal),
    staleTime: Infinity,
  })

/** The cached session, or a signed-out one. */
export const currentSession = (): SessionState =>
  queryClient.getQueryData<SessionState>(queryKeys.session) ?? signedOut

/** Changes the cached session in place, without a request. */
export function patchSession(patch: Partial<SessionState>): void {
  queryClient.setQueryData<SessionState>(queryKeys.session, (session) => ({ ...(session ?? signedOut), ...patch }))
}

/** The signed-in account, or `null`, and what the session may do. */
export function useSession() {
  const query = useQuery({
    queryKey: queryKeys.session,
    queryFn: ({ signal }) => sessionResource.current(signal),
    staleTime: Infinity,
  })
  const account = computed(() => query.data.value?.account ?? null)
  return {
    account,
    enrollmentOnly: computed(() => query.data.value?.enrollment_only ?? false),
    isInstallAdmin: computed(() => account.value?.install_role === 'install_admin'),
  }
}

/** Drops every cached result and closes every live stream, so nothing of the previous account stays. */
export function forgetEverything(): void {
  closeAllLive()
  void queryClient.cancelQueries()
  queryClient.clear()
}

/**
 * Applies a login step. A finished login replaces everything cached with the new account; the
 * second-factor step changes nothing yet. Returns where the person goes next.
 */
export function applyLoginResult(result: LoginResult): 'signed_in' | 'second_factor' | 'enroll' {
  if (result.status === 'second_factor_required') return 'second_factor'
  forgetEverything()
  const enrollmentOnly = result.status === 'enrollment_required'
  queryClient.setQueryData<SessionState>(queryKeys.session, {
    account: result.account,
    enrollment_only: enrollmentOnly,
  })
  return enrollmentOnly ? 'enroll' : 'signed_in'
}

/** Ends the session server-side, then clears restricted caches and shows the sign-in page. */
export function useSignOut() {
  const router = useRouter()
  return async (): Promise<void> => {
    try {
      await sessionResource.logout()
    } finally {
      forgetEverything()
      queryClient.setQueryData<SessionState>(queryKeys.session, signedOut)
      await router.push({ name: 'login' })
    }
  }
}
