import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'

import { sitesResource } from '~/api/resources/sites'
import { queryKeys } from '~/api/query-keys'
import { loadSession } from '~/composables/useSession'
import { queryClient } from '~/plugins/vue-query'
import { runtimeConfig } from '~/services/runtime-config'

declare module 'vue-router' {
  interface RouteMeta {
    /** Signed-out pages render without the shell; signed-in visitors bounce home. */
    guest?: boolean
    /** The one page an enrollment-only session may open (decision 0007). Renders without the shell. */
    enroll?: boolean
    /**
     * The setup wizard (decision 0021). In setup mode it is the only page; afterwards it shows
     * the new admin's first-site step.
     */
    setup?: boolean
    /** Read through a share token, without the person's session or the app shell. */
    share?: boolean
    /** Installation administration; other people get the not-found page. */
    installAdmin?: boolean
    /** The navigation item a detail page belongs to, so the sidebar marks it. */
    section?: string
  }
}

const routes: RouteRecordRaw[] = [
  { path: '/login', name: 'login', component: () => import('~/pages/LoginPage.vue'), meta: { guest: true } },
  {
    path: '/login/two-factor',
    name: 'login-two-factor',
    component: () => import('~/pages/auth/TwoFactorChallengePage.vue'),
    meta: { guest: true },
  },
  { path: '/setup', name: 'setup', component: () => import('~/pages/setup/SetupPage.vue'), meta: { setup: true } },
  { path: '/login/enroll', name: 'enroll', component: () => import('~/pages/auth/EnrollPage.vue'), meta: { enroll: true } },
  {
    path: '/share/:token',
    component: () => import('~/pages/share/SharePage.vue'),
    meta: { share: true },
    children: [
      { path: '', name: 'share', component: () => import('~/pages/share/ShareView.vue') },
      { path: 'replays', name: 'share-replays', component: () => import('~/pages/share/ShareReplays.vue') },
      { path: 'replays/:captureId', name: 'share-replay', component: () => import('~/pages/share/ShareReplay.vue') },
      // Imported Umami links: 2.x `/share/<id>/<website name>`, 3.x `/share/<slug>/<section>`.
      { path: ':legacy(.*)*', redirect: to => ({ name: 'share', params: { token: to.params.token } }) },
    ],
  },
  { path: '/', name: 'home', component: () => import('~/pages/HomePage.vue') },
  {
    path: '/workspaces/:workspaceId',
    component: () => import('~/pages/WorkspacePage.vue'),
    children: [
      { path: '', name: 'workspace', component: () => import('~/pages/workspace/WorkspaceSites.vue') },
      { path: 'links', name: 'workspace-links', component: () => import('~/pages/workspace/LinksSection.vue') },
      { path: 'members', name: 'workspace-members', component: () => import('~/pages/workspace/MembersSection.vue') },
      { path: 'settings', name: 'workspace-settings', component: () => import('~/pages/workspace/TeamSettings.vue') },
    ],
  },
  {
    path: '/workspaces/:workspaceId/links/:sourceId',
    name: 'link-report',
    component: () => import('~/pages/links/SourceReportPage.vue'),
    props: { kind: 'link' },
  },
  {
    path: '/workspaces/:workspaceId/pixels/:sourceId',
    name: 'pixel-report',
    component: () => import('~/pages/links/SourceReportPage.vue'),
    props: { kind: 'pixel' },
  },
  { path: '/teams/join', name: 'team-join', component: () => import('~/pages/workspace/JoinTeamPage.vue') },
  { path: '/sites/:siteId', name: 'overview', component: () => import('~/pages/OverviewPage.vue') },
  {
    path: '/sites/:siteId/settings',
    component: () => import('~/pages/SiteSettingsPage.vue'),
    children: [
      { path: '', name: 'site-settings', component: () => import('~/pages/settings/SiteSettingsForm.vue') },
      { path: 'tracking', name: 'site-settings-tracking', component: () => import('~/pages/settings/TrackingSection.vue') },
      { path: 'sessions', name: 'site-settings-sessions', component: () => import('~/pages/settings/SessionPolicyForm.vue') },
      { path: 'identity', name: 'site-settings-identity', component: () => import('~/pages/settings/IdentityPolicyForm.vue') },
      { path: 'retention', name: 'site-settings-retention', component: () => import('~/pages/settings/RetentionForm.vue') },
      { path: 'recording', name: 'recording-settings', component: () => import('~/pages/settings/RecordingSettingsSection.vue') },
      { path: 'sharing', name: 'site-settings-sharing', component: () => import('~/pages/settings/SharingSection.vue') },
    ],
  },
  { path: '/sites/:siteId/replays', name: 'replays', component: () => import('~/pages/ReplaysPage.vue') },
  { path: '/sites/:siteId/replays/:captureId', name: 'replay', component: () => import('~/pages/ReplayPage.vue') },
  { path: '/sites/:siteId/heatmaps', name: 'heatmaps', component: () => import('~/pages/HeatmapsPage.vue') },
  { path: '/sites/:siteId/explore', name: 'explore', component: () => import('~/pages/explore/ExplorePage.vue') },
  { path: '/sites/:siteId/realtime', name: 'realtime', component: () => import('~/pages/realtime/RealtimePage.vue') },
  { path: '/sites/:siteId/events', name: 'events', component: () => import('~/pages/events/EventsPage.vue') },
  { path: '/sites/:siteId/funnels', name: 'funnels', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/goals', name: 'goals', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/journeys', name: 'journeys', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/campaigns', name: 'campaigns', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/attribution', name: 'attribution', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/sessions', name: 'sessions', component: () => import('~/pages/reports/ReportPage.vue') },
  { path: '/sites/:siteId/sessions/:sessionId', name: 'session', component: () => import('~/pages/sessions/SessionPage.vue'), meta: { section: 'sessions' } },
  { path: '/sites/:siteId/retention', name: 'retention', component: () => import('~/pages/retention/RetentionPage.vue') },
  { path: '/sites/:siteId/revenue', name: 'revenue', component: () => import('~/pages/revenue/RevenuePage.vue') },
  { path: '/sites/:siteId/performance', name: 'performance', component: () => import('~/pages/performance/PerformancePage.vue') },
  { path: '/boards', name: 'boards', component: () => import('~/pages/boards/BoardsPage.vue') },
  { path: '/boards/:boardId', name: 'board', component: () => import('~/pages/boards/BoardPage.vue') },
  {
    path: '/account',
    component: () => import('~/pages/AccountPage.vue'),
    children: [
      { path: '', name: 'account', redirect: { name: 'account-profile' } },
      { path: 'profile', name: 'account-profile', component: () => import('~/pages/account/ProfilePage.vue') },
      { path: 'preferences', name: 'account-preferences', component: () => import('~/pages/account/PreferencesPage.vue') },
      { path: 'security', name: 'account-security', component: () => import('~/pages/account/SecurityPage.vue') },
      { path: 'api-keys', name: 'account-api-keys', component: () => import('~/pages/account/ApiKeysPage.vue') },
    ],
  },
  {
    path: '/admin',
    component: () => import('~/pages/admin/AdminPage.vue'),
    meta: { installAdmin: true },
    children: [
      { path: '', name: 'admin', redirect: { name: 'admin-users' } },
      { path: 'users', name: 'admin-users', component: () => import('~/pages/admin/UsersPage.vue') },
      { path: 'security', name: 'admin-security', component: () => import('~/pages/admin/SecurityPolicyPage.vue') },
      { path: 'status', name: 'admin-status', component: () => import('~/pages/admin/StatusPage.vue') },
    ],
  },
  { path: '/:path(.*)*', name: 'not-found', component: () => import('~/pages/NotFoundPage.vue') },
]

export const router = createRouter({
  history: createWebHistory(),
  routes,
})

router.beforeEach(async (to) => {
  // Until an admin exists, nothing but the wizard answers.
  if (runtimeConfig.setup) return to.meta.setup ? true : { name: 'setup', replace: true }
  if (to.meta.share) return true
  const { account, enrollment_only } = await loadSession().catch(() => ({ account: null, enrollment_only: false }))

  if (to.meta.guest) return account ? (enrollment_only ? { name: 'enroll' } : { name: 'home' }) : true
  if (!account) return { name: 'login', query: to.fullPath === '/' ? {} : { redirect: to.fullPath } }
  // Until a second factor is confirmed, the enrollment page is the only one there is.
  if (enrollment_only) return to.meta.enroll ? true : { name: 'enroll', query: { redirect: to.fullPath } }
  if (to.meta.enroll) return { name: 'home' }
  if (to.meta.installAdmin && account.install_role !== 'install_admin') {
    return { name: 'not-found', params: { path: to.path.slice(1).split('/') } }
  }

  // Home opens the first site the caller may see, or the first workspace when it has none.
  if (to.name === 'home') {
    const { workspaces } = await queryClient.ensureQueryData({
      queryKey: queryKeys.workspaces,
      queryFn: ({ signal }) => sitesResource.workspaces(signal),
    })
    const site = workspaces.flatMap((workspace) => workspace.sites)[0]
    if (site) return { name: 'overview', params: { siteId: site.id }, replace: true }
    const workspace = workspaces[0]
    if (workspace) return { name: 'workspace', params: { workspaceId: workspace.id }, replace: true }
  }
  return true
})
