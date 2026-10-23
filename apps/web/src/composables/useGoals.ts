import { useMutation, useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { goalsResource } from '~/api/resources/goals'
import type { GoalDefinition, GoalId } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { watchScope } from './useExplore'

/** The site's active goals at their latest version, with save (a new version) and archive. */
export function useGoals(site: MaybeRefOrGetter<string>) {
  const list = useQuery({
    queryKey: computed(() => queryKeys.goals(toValue(site))),
    queryFn: ({ signal }) => goalsResource.list(toValue(site), signal),
  })
  watchScope(site, () => list.error.value)
  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.goals(toValue(site)) })
  const save = useMutation({
    mutationFn: (args: { name: string; definition: GoalDefinition; goal?: GoalId }) =>
      goalsResource.save(toValue(site), args.name, args.definition, args.goal),
    onSuccess: refresh,
  })
  const archive = useMutation({
    mutationFn: (goal: GoalId) => goalsResource.archive(toValue(site), goal),
    onSuccess: refresh,
  })
  return { list, save, archive, goals: computed(() => list.data.value?.goals ?? []) }
}
