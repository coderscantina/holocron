import { api } from '~/api'
import { currentSession, forgetEverything, patchSession } from '~/composables/useSession'
import { router } from '~/router'

/**
 * Session-wide reactions to error answers:
 *
 * - `unauthenticated` means the session ended elsewhere (expired, revoked, signed out in another
 *   tab). Drop every restricted result and show the sign-in page once. `invalid_credentials` and
 *   `second_factor_required` from `auth.reauthenticate` are 401s too, but do not end the session.
 * - `second_factor_enrollment_required` means a 2FA requirement now applies: the session may
 *   only enroll, so everything else is dropped and the enrollment page opens.
 * - `reauthentication_required` means the password check lapsed; the next sensitive command
 *   asks again.
 */
export function installAuthHandler(): void {
  api.setAuthHandler(() => {
    forgetEverything()
    const current = router.currentRoute.value
    // The setup wizard signs the admin in again on its own page.
    if (current.meta.guest || current.meta.share || current.meta.setup) return
    void router.push({ name: 'login', query: { redirect: current.fullPath } })
  })
  api.onError((error) => {
    if (error.code === 'reauthentication_required') patchSession({ reauthenticated_until: undefined })
    if (error.code === 'second_factor_enrollment_required' && !router.currentRoute.value.meta.enroll) {
      const { account } = currentSession()
      forgetEverything()
      patchSession({ account, enrollment_only: true })
      void router.push({ name: 'enroll' })
    }
  })
}
