import type { LoginArgs, ReauthenticateArgs, SecondFactorProof } from '~/generated'

import { api } from '..'

/** Sign-in state. `current` never fails with `unauthenticated`; a signed-out caller gets `account: null`. */
export const sessionResource = {
  current: (signal?: AbortSignal) => api.query('auth.session', {}, { signal }),
  /** A correct password signs in, asks for the second factor, or opens an enrollment-only session. */
  login: (args: LoginArgs) => api.command('auth.login', args),
  /** The second step; the challenge travels as a cookie from `login`. */
  verifySecondFactor: (proof: SecondFactorProof) => api.command('auth.verifySecondFactor', proof),
  logout: () => api.command('auth.logout', {}),
  /** Marks the session for sensitive commands. Wrong input answers 401 without ending the session. */
  reauthenticate: (args: ReauthenticateArgs) => api.command('auth.reauthenticate', args),
  /** Ends every other session of the person; this one stays. */
  revokeOthers: () => api.command('auth.revokeSessions', {}),
}
