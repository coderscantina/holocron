import type { Commands } from '~/generated/commands'
import { REAUTHENTICATION_WINDOW_MS } from '~/generated/limits'

/** Commands the registry marks as needing a fresh password check (decision 0007). */
export type ReauthCommand = {
  [N in keyof Commands]: Commands[N]['reauth'] extends 'never' ? never : N
}[keyof Commands]

const always = () => true
/** The server decides from the account, not the arguments; `useReauth` asks when it refuses. */
const byServer = () => false

/**
 * Whether a call needs the check. Exhaustive over the generated `reauth` metadata: a new
 * sensitive command is a compile error here until it has a rule. `handler` commands decide by
 * their arguments where they can.
 */
export const reauthRules: { [N in ReauthCommand]: (args: Commands[N]['args']) => boolean } = {
  'account.changePassword': always,
  'apiKeys.create': always,
  'links.delete': always,
  'members.updateRole': (args) => args.role === 'admin' || args.role === 'owner',
  'pixels.delete': always,
  // Only shortening needs the check; the server knows the current value.
  'retention.updateDefault': byServer,
  'security.updatePolicy': always,
  'sites.delete': always,
  'sites.reset': always,
  'sites.transfer': always,
  'sites.updateRetention': byServer,
  'teams.delete': always,
  // Only replacing a confirmed factor needs the check (decision 0007).
  'twoFactor.begin': byServer,
  'twoFactor.confirm': byServer,
  'twoFactor.disable': always,
  'twoFactor.regenerateRecoveryCodes': always,
  'users.delete': always,
  'users.resetCredentials': always,
}

/** A check this close to running out is renewed first, so it cannot lapse in flight. */
const MARGIN_MS = 30_000

/** Whether `name` with `args` needs a password prompt before it runs. */
export function needsPrompt<N extends ReauthCommand>(
  name: N,
  args: Commands[N]['args'],
  reauthenticatedUntil: number | undefined,
  now = Date.now()
): boolean {
  if (!reauthRules[name](args)) return false
  return !reauthenticatedUntil || reauthenticatedUntil - now <= MARGIN_MS
}

/** The window the server grants, for display. */
export const REAUTH_MINUTES = REAUTHENTICATION_WINDOW_MS / 60_000
