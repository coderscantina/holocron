import type {
  LoginResult,
  SetupAdminArgs,
  SetupCheck,
  SetupDiscardArgs,
  SetupImportProgress,
  SetupInspectArgs,
  SetupInspection,
  SetupRunArgs,
  SetupSignInArgs,
  SetupStatus,
  SetupTokenArgs,
} from '~/generated'

import { api } from '..'

/** The setup wizard's endpoints. They answer only while the server is in setup mode. */
export const setupResource = {
  /** Where setup stands; in the import phase with the signed-in admin's import. */
  status: (signal?: AbortSignal) => api.get<SetupStatus>('setup.status', '/setup/status', signal),
  /** Checks the token and reports what the server sees of its deployment. */
  check: (args: SetupTokenArgs) => api.post<SetupCheck>('setup.check', '/setup/check', args),
  /** Creates the install admin and signs it in. On the fresh path the server leaves setup mode right after. */
  admin: (args: SetupAdminArgs) => api.post<LoginResult>('setup.admin', '/setup/admin', args),
  /** Signs the install admin in again while an import waits, for example after a restart. */
  signIn: (args: SetupSignInArgs) => api.post<LoginResult>('setup.signIn', '/setup/sign-in', args),
  /** The inspect preview of an Umami database, or the refusal with its fix. */
  inspect: (args: SetupInspectArgs) => api.post<SetupInspection>('setup.inspect', '/setup/import/inspect', args),
  /** Starts or resumes the import; progress follows on [`progress`]. */
  run: (args: SetupRunArgs) => api.post<SetupImportProgress>('setup.run', '/setup/import/run', args),
  /** Calls `onFrame` with the current progress, then with each change. Returns the close function. */
  progress: (onFrame: (frame: SetupImportProgress) => void, onEnd: () => void) =>
    api.stream('/setup/import/progress', (data) => onFrame(JSON.parse(data) as SetupImportProgress), onEnd),
  /** Where the browser downloads the manifest of a complete import. */
  manifestUrl: () => api.url('/setup/import/manifest'),
  /** Removes the imported data; the admin and the secret key stay. */
  discard: (args: SetupDiscardArgs) => api.post<object>('setup.discard', '/setup/import/discard', args),
  /** Acknowledges the report; the server leaves setup mode right after. */
  finish: () => api.post<object>('setup.finish', '/setup/finish', {}),
}
