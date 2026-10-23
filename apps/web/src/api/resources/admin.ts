import type { SecurityUpdatePolicyArgs, UserId, UsersCreateArgs, UsersResetCredentialsArgs, UsersUpdateArgs } from '~/generated'

import { api } from '..'

/** Installation administration: accounts, the 2FA requirement policy, the server status and the GeoIP database. */
export const adminResource = {
  users: (signal?: AbortSignal) => api.query('users.list', {}, { signal }),
  /** Discloses the generated password once, in `secret`. */
  createUser: (args: UsersCreateArgs, idempotencyKey?: string) => api.command('users.create', args, { idempotencyKey }),
  updateUser: (args: UsersUpdateArgs) => api.command('users.update', args),
  deleteUser: (user: UserId) => api.command('users.delete', { user }),
  resetCredentials: (args: UsersResetCredentialsArgs) => api.command('users.resetCredentials', args),
  securityPolicy: (signal?: AbortSignal) => api.query('security.policy', {}, { signal }),
  updateSecurityPolicy: (args: SecurityUpdatePolicyArgs) => api.command('security.updatePolicy', args),
  operatorStatus: (signal?: AbortSignal) => api.query('operator.status', {}, { signal }),
  geoipSettings: (signal?: AbortSignal) => api.query('geoip.settings', {}, { signal }),
  /** Starts the DB-IP download and answers at once; the settings tell when it is done. */
  downloadGeoip: () => api.command('geoip.download', {}),
}
