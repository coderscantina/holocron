import type { RecalculationId, SiteId, WorkspaceId } from '~/generated'

/**
 * Vue Query keys start with the operation name and include every argument. Restricted results
 * are dropped from the cache on sign-in, sign-out and any `unauthenticated` answer. Share
 * viewer keys start with `share` and the token instead, so they never mix with the person's own.
 */
export const queryKeys = {
  session: ['auth.session'] as const,
  workspaces: ['workspaces.list'] as const,
  account: ['account.get'] as const,
  preferences: ['account.preferences'] as const,
  twoFactor: ['twoFactor.status'] as const,
  apiKeys: ['apiKeys.list'] as const,
  sites: ['sites.list'] as const,
  /** Workspace-scoped keys put the workspace second, so a lost membership drops them together. */
  team: (workspace: WorkspaceId) => ['teams.get', workspace] as const,
  invitations: (workspace: WorkspaceId) => ['teams.invitations', workspace] as const,
  shares: (site: SiteId) => ['sharing.list', site] as const,
  users: ['users.list'] as const,
  retentionSettings: ['retention.settings'] as const,
  securityPolicy: ['security.policy'] as const,
  operatorStatus: ['operator.status'] as const,
  geoipSettings: ['geoip.settings'] as const,
  /** Everything read through one share token. */
  share: (token: string) => ['share', token] as const,
  shareView: (token: string) => ['share', token, 'sharing.resolve'] as const,

  site: (site: SiteId) => ['sites.get', site] as const,
  identityPolicy: (site: SiteId) => ['sites.identityPolicy', site] as const,
  trackingCode: (site: SiteId, origin: string) => ['sites.trackingCode', site, origin] as const,
  collectionKeys: (site: SiteId) => ['collectionKeys.list', site] as const,
  recalculation: (site: SiteId, recalculation: RecalculationId) => ['recalculations.status', site, recalculation] as const,
  /** Site-scoped analytics keys put the site second, so a revoked scope drops them together. */
  explore: (site: SiteId, state: string, panels: string, epoch: number) =>
    ['analytics.explore', site, state, panels, epoch] as const,
  catalog: (site: SiteId) => ['fields.catalog', site] as const,
  values: (site: SiteId, field: string) => ['fields.values', site, field] as const,
  realtime: (site: SiteId) => ['realtime.snapshot', site] as const,
  sessionDetail: (site: SiteId, session: number) => ['sessions.get', site, session] as const,
  sessionTimeline: (site: SiteId, hours: number) => ['sessions.timeline', site, hours] as const,
  timelineReplays: (site: SiteId, hours: number) => ['recordings.list', site, 'timeline', hours] as const,
  saved: (site: SiteId) => ['explorations.list', site] as const,
  segments: (site: SiteId) => ['segments.list', site] as const,
  goals: (site: SiteId) => ['goals.list', site] as const,
  annotations: (site: SiteId, start: number, end: number) => ['annotations.list', site, start, end] as const,
  boards: (workspace: WorkspaceId) => ['boards.list', workspace] as const,
  links: (workspace: WorkspaceId) => ['links.list', workspace] as const,
  pixels: (workspace: WorkspaceId) => ['pixels.list', workspace] as const,
  /** A link's or pixel's report; workspace-scoped like the lists. */
  sourceExplore: (workspace: WorkspaceId, kind: string, state: string, panels: string, epoch: number) =>
    ['links.explore', workspace, kind, state, panels, epoch] as const,
  /** Summaries of the sites in view; the workspace second drops them with a lost membership. */
  summaries: (workspace: WorkspaceId, sites: readonly SiteId[]) => ['summaries.forSites', workspace, ...sites] as const,
  /** A site's findings; site-scoped, so a revoked scope drops them with the rest. */
  findings: (site: SiteId, limit: number) => ['summaries.findings', site, limit] as const,
  board: (board: string) => ['boards.get', board] as const,
}
