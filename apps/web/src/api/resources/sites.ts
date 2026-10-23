import type {
  CollectionKeyId,
  CollectionKeysCreateArgs,
  RecalculationId,
  RecalculationsStartArgs,
  SiteId,
  SitesCreateArgs,
  SitesTransferArgs,
  SitesUpdateArgs,
  SitesUpdateIdentityPolicyArgs,
  SitesUpdateRetentionArgs,
  SitesUpdateSessionPolicyArgs,
} from '~/generated'

import { api } from '..'

export const sitesResource = {
  /** The header's context options: workspaces the caller belongs to, with their sites. */
  workspaces: (signal?: AbortSignal) => api.query('workspaces.list', {}, { signal }),
  /** Every site the caller can reach, with its workspace's name. */
  list: (signal?: AbortSignal) => api.query('sites.list', {}, { signal }),
  /** One site with the caller's capabilities. `revision` is the value to send as `expected_revision`. */
  get: (site: SiteId, signal?: AbortSignal) => api.query('sites.get', { site }, { signal }),
  create: (args: SitesCreateArgs, idempotencyKey?: string) => api.command('sites.create', args, { idempotencyKey }),
  transfer: (args: SitesTransferArgs) => api.command('sites.transfer', args),
  /** Deletes the site with all its data. */
  remove: (site: SiteId) => api.command('sites.delete', { site }),
  /** Deletes the site's collected data and keeps its settings. */
  reset: (site: SiteId) => api.command('sites.reset', { site }),
  update: (args: SitesUpdateArgs, idempotencyKey?: string) =>
    api.command('sites.update', args, { idempotencyKey }),
  /** Inactivity window and timezone. A change starts a new session policy version. */
  updateSessionPolicy: (args: SitesUpdateSessionPolicyArgs, idempotencyKey?: string) =>
    api.command('sites.updateSessionPolicy', args, { idempotencyKey }),
  identityPolicy: (site: SiteId, signal?: AbortSignal) => api.query('sites.identityPolicy', { site }, { signal }),
  updateIdentityPolicy: (args: SitesUpdateIdentityPolicyArgs, idempotencyKey?: string) =>
    api.command('sites.updateIdentityPolicy', args, { idempotencyKey }),
  /** What `updateRetention` would remove now. */
  retentionPreview: (args: SitesUpdateRetentionArgs) => api.query('sites.retentionPreview', args),
  /** `retention` absent makes the site follow the installation default. Shortening needs a fresh login. */
  updateRetention: (args: SitesUpdateRetentionArgs, idempotencyKey?: string) =>
    api.command('sites.updateRetention', args, { idempotencyKey }),
  recalculate: (args: RecalculationsStartArgs, idempotencyKey?: string) =>
    api.command('recalculations.start', args, { idempotencyKey }),
  recalculation: (site: SiteId, recalculation: RecalculationId, signal?: AbortSignal) =>
    api.query('recalculations.status', { site, recalculation }, { signal }),
  /** Script tags for the site. `origin` stands in when the server has no public URL configured. */
  trackingCode: (site: SiteId, origin: string, signal?: AbortSignal) =>
    api.query('sites.trackingCode', { site, origin }, { signal }),
  collectionKeys: (site: SiteId, signal?: AbortSignal) => api.query('collectionKeys.list', { site }, { signal }),
  /** Discloses the key's secret once, in `secret`. */
  createCollectionKey: (args: CollectionKeysCreateArgs, idempotencyKey?: string) =>
    api.command('collectionKeys.create', args, { idempotencyKey }),
  revokeCollectionKey: (site: SiteId, key: CollectionKeyId) => api.command('collectionKeys.revoke', { site, key }),
}
