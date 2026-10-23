import { useMutation, useQuery } from '@tanstack/vue-query'
import { createSharedComposable } from '@vueuse/core'
import { computed, ref, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import type { SiteSummary, SitesUpdateArgs, WorkspaceSummary } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

const siteRoutes = new Set([
  'overview',
  'explore',
  'realtime',
  'events',
  'site-settings',
  'site-settings-tracking',
  'site-settings-sessions',
  'site-settings-identity',
  'recording-settings',
  'site-settings-sharing',
])

/** The last site a route named, so workspace-wide pages keep the header context. */
const lastSiteId = ref<string | null>(null)

export function useWorkspaces() {
  return useQuery({
    queryKey: queryKeys.workspaces,
    queryFn: ({ signal }) => sitesResource.workspaces(signal),
  })
}

/**
 * The header context: the workspace and site named by the route (or the last site visited, on
 * pages without one), resolved against the server-authorized options only. A site ID the caller
 * cannot see resolves to nothing.
 */
export const useSiteContext = createSharedComposable(() => {
  const route = useRoute()
  const router = useRouter()
  const workspacesQuery = useWorkspaces()

  const param = (name: string) => {
    const value = route.params[name]
    return typeof value === 'string' ? value : null
  }
  watchEffect(() => {
    const id = param('siteId')
    if (id) lastSiteId.value = id
  })
  const siteId = computed(() => param('siteId') ?? (param('workspaceId') ? null : lastSiteId.value))
  const workspaces = computed(() => workspacesQuery.data.value?.workspaces ?? [])

  const workspace = computed<WorkspaceSummary | null>(() => {
    const id = siteId.value
    if (id) return workspaces.value.find((entry) => entry.sites.some((site) => site.id === id)) ?? null
    return workspaces.value.find((entry) => entry.id === param('workspaceId')) ?? null
  })
  const site = computed<SiteSummary | null>(
    () => workspace.value?.sites.find((entry) => entry.id === siteId.value) ?? null
  )

  /** Keeps the current section when it exists for every site; otherwise opens the overview. */
  const selectSite = (next: SiteSummary) => {
    const name = typeof route.name === 'string' && siteRoutes.has(route.name) ? route.name : 'overview'
    return router.push({ name, params: { siteId: next.id } })
  }

  const selectWorkspace = (next: WorkspaceSummary) => {
    const first = next.sites[0]
    return first ? selectSite(first) : router.push({ name: 'workspace', params: { workspaceId: next.id } })
  }

  return {
    workspaces,
    workspace,
    site,
    siteId,
    loading: computed(() => workspacesQuery.isPending.value),
    selectSite,
    selectWorkspace,
  }
})

/** One site's settings and the caller's capabilities on it. */
export function useSite(site: MaybeRefOrGetter<string>) {
  return useQuery({
    queryKey: computed(() => queryKeys.site(toValue(site))),
    queryFn: ({ signal }) => sitesResource.get(toValue(site), signal),
  })
}

/**
 * Renames a site. Afterwards the site snapshot is read again, because its cursor is the
 * server's to hand out, and the header options pick up the new name.
 */
export function useUpdateSite() {
  return useMutation({
    mutationFn: ({ args, idempotencyKey }: { args: SitesUpdateArgs; idempotencyKey: string }) =>
      sitesResource.update(args, idempotencyKey),
    onSuccess: (answer) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.site(answer.site.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ]),
  })
}

/** Every site the caller can reach, across workspaces. */
export function useSiteList() {
  return useQuery({
    queryKey: queryKeys.sites,
    queryFn: ({ signal }) => sitesResource.list(signal),
  })
}
