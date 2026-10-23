import type { ShareId, SharingCreateArgs, SiteId } from '~/generated'
import { SHARE_SCHEME } from '~/generated/limits'

import { api } from '..'
import type { ApiClient } from '../client'

/** Restricted shares of a site, as their author manages them. */
export const sharingResource = {
  list: (site: SiteId, signal?: AbortSignal) => api.query('sharing.list', { site }, { signal }),
  /** Discloses the viewer token once, in `secret`. */
  create: (args: SharingCreateArgs, idempotencyKey?: string) => api.command('sharing.create', args, { idempotencyKey }),
  revoke: (share: ShareId) => api.command('sharing.revoke', { share }),
}

/** A transport that reads as the share, never as the signed-in person (decision 0012). */
export const shareClient = (token: string): ApiClient => api.as(`${SHARE_SCHEME} ${token}`)

/**
 * Reads made for a share viewer: through the share's client, cached under the token. Composables
 * that take one prefix their query keys with `['share', token]`, so share results never mix
 * with the person's own and drop together when the share goes away.
 */
export interface ShareScope {
  token: string
  client: ApiClient
}

export const shareKey = <K extends readonly unknown[]>(scope: ShareScope | undefined, key: K) =>
  scope ? (['share', scope.token, ...key] as const) : key

/** What a share viewer reads, through the share's own client. */
export const sharedViewResource = {
  resolve: (client: ApiClient, signal?: AbortSignal) => client.query('sharing.resolve', {}, { signal }),
  board: (client: ApiClient, board: string, signal?: AbortSignal) => client.query('boards.get', { board }, { signal }),
}
