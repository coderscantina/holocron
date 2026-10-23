import { useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { linksResource } from '~/api/resources/links'
import type { WorkspaceId } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

/** The links and pixels of a workspace, each list with what the caller may do with them. */
export function useLinks(workspace: MaybeRefOrGetter<WorkspaceId>) {
  const links = useQuery({
    queryKey: computed(() => queryKeys.links(toValue(workspace))),
    queryFn: ({ signal }) => linksResource.links(toValue(workspace), signal),
  })
  const pixels = useQuery({
    queryKey: computed(() => queryKeys.pixels(toValue(workspace))),
    queryFn: ({ signal }) => linksResource.pixels(toValue(workspace), signal),
  })
  return { links, pixels }
}

/** Reads both lists again after a change. */
export const refreshLinks = (workspace: WorkspaceId) =>
  Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.links(workspace) }),
    queryClient.invalidateQueries({ queryKey: queryKeys.pixels(workspace) }),
  ])
