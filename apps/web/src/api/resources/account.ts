import type { AccountUpdateArgs, ApiKeyId, ApiKeysCreateArgs, ChangePasswordArgs, Preferences } from '~/generated'

import { api } from '..'

/** The signed-in person's own profile, preferences, second factor and API keys. */
export const accountResource = {
  get: (signal?: AbortSignal) => api.query('account.get', {}, { signal }),
  update: (args: AccountUpdateArgs, idempotencyKey?: string) => api.command('account.update', args, { idempotencyKey }),
  changePassword: (args: ChangePasswordArgs) => api.command('account.changePassword', args),
  preferences: (signal?: AbortSignal) => api.query('account.preferences', {}, { signal }),
  updatePreferences: (args: Preferences) => api.command('account.updatePreferences', args),

  twoFactor: (signal?: AbortSignal) => api.query('twoFactor.status', {}, { signal }),
  /** A new pending secret; the active factor, if any, stays until `confirm`. */
  beginTwoFactor: () => api.command('twoFactor.begin', {}),
  confirmTwoFactor: (code: string) => api.command('twoFactor.confirm', { code }),
  disableTwoFactor: () => api.command('twoFactor.disable', {}),
  regenerateRecoveryCodes: () => api.command('twoFactor.regenerateRecoveryCodes', {}),

  apiKeys: (signal?: AbortSignal) => api.query('apiKeys.list', {}, { signal }),
  createApiKey: (args: ApiKeysCreateArgs, idempotencyKey?: string) =>
    api.command('apiKeys.create', args, { idempotencyKey }),
  revokeApiKey: (key: ApiKeyId) => api.command('apiKeys.revoke', { key }),
}
