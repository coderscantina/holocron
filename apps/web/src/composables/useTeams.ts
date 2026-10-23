import { useQuery } from '@tanstack/vue-query'
import { computed, toValue, watch, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { teamsResource } from '~/api/resources/teams'
import type { WorkspaceCapability, WorkspaceId } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { isRefusal } from './useExplore'

/**
 * Drops every cached result of a workspace after the caller lost it (removed, left, deleted),
 * and reads the header options again, so the workspace leaves the selectors too.
 */
export function forgetWorkspace(workspace: WorkspaceId): void {
  const ofWorkspace = (key: readonly unknown[]) => key[1] === workspace
  void queryClient.cancelQueries({ predicate: (query) => ofWorkspace(query.queryKey) })
  queryClient.removeQueries({ predicate: (query) => ofWorkspace(query.queryKey) })
  void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
  void queryClient.invalidateQueries({ queryKey: queryKeys.sites })
}

/** A team or personal workspace with its members, and what the caller may do in it. */
export function useTeam(workspace: MaybeRefOrGetter<WorkspaceId>) {
  const query = useQuery({
    queryKey: computed(() => queryKeys.team(toValue(workspace))),
    queryFn: ({ signal }) => teamsResource.get(toValue(workspace), signal),
  })
  watch(
    () => query.error.value,
    (error) => {
      if (isRefusal(error)) forgetWorkspace(toValue(workspace))
    }
  )
  const can = (capability: WorkspaceCapability) => query.data.value?.team.capabilities.includes(capability) ?? false
  return { query, can }
}

/** Reads a workspace's member list and header options again after a membership change. */
export const refreshTeam = (workspace: WorkspaceId) =>
  Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.team(workspace) }),
    queryClient.invalidateQueries({ queryKey: queryKeys.invitations(workspace) }),
    queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
  ])
