import { useMutation, useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { boardsResource } from '~/api/resources/boards'
import { explorationsResource } from '~/api/resources/explorations'
import type { BoardsUpdateArgs, ExplorationState, Group } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { watchScope } from './useExplore'

/** The caller's saved explorations of a site, with save and delete. */
export function useSavedExplorations(site: MaybeRefOrGetter<string>) {
  const list = useQuery({
    queryKey: computed(() => queryKeys.saved(toValue(site))),
    queryFn: ({ signal }) => explorationsResource.saved(toValue(site), signal),
  })
  watchScope(site, () => list.error.value)
  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.saved(toValue(site)) })
  const save = useMutation({
    mutationFn: (args: { name: string; state: ExplorationState; exploration?: string }) =>
      explorationsResource.save(toValue(site), args.name, args.state, args.exploration),
    onSuccess: refresh,
  })
  const remove = useMutation({
    mutationFn: (exploration: string) => explorationsResource.remove(toValue(site), exploration),
    onSuccess: refresh,
  })
  return { list, save, remove }
}

/** The site's segments, with save and delete for roles that write definitions. */
export function useSegments(site: MaybeRefOrGetter<string>) {
  const list = useQuery({
    queryKey: computed(() => queryKeys.segments(toValue(site))),
    queryFn: ({ signal }) => explorationsResource.segments(toValue(site), signal),
  })
  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.segments(toValue(site)) })
  const save = useMutation({
    mutationFn: (args: { name: string; filter: Group }) =>
      explorationsResource.saveSegment(toValue(site), args.name, args.filter),
    onSuccess: refresh,
  })
  const remove = useMutation({
    mutationFn: (segment: string) => explorationsResource.removeSegment(toValue(site), segment),
    onSuccess: refresh,
  })
  return { list, save, remove }
}

/** Annotations of a site inside the resolved range on screen. */
export function useAnnotations(site: MaybeRefOrGetter<string>, range: MaybeRefOrGetter<{ start: number; end: number } | null>) {
  const list = useQuery({
    queryKey: computed(() => {
      const bounds = toValue(range)
      return queryKeys.annotations(toValue(site), bounds?.start ?? 0, bounds?.end ?? 0)
    }),
    queryFn: ({ signal }) => {
      const bounds = toValue(range)
      if (!bounds) throw new Error('no range')
      return explorationsResource.annotations(toValue(site), bounds.start, bounds.end, signal)
    },
    enabled: computed(() => toValue(range) !== null),
  })
  const refresh = () =>
    queryClient.invalidateQueries({ predicate: (query) => query.queryKey[0] === 'annotations.list' && query.queryKey[1] === toValue(site) })
  const create = useMutation({
    mutationFn: (args: { at: number; text: string }) =>
      explorationsResource.annotate({ site: toValue(site), at: args.at, text: args.text }),
    onSuccess: refresh,
  })
  const remove = useMutation({
    mutationFn: (annotation: string) => explorationsResource.removeAnnotation(toValue(site), annotation),
    onSuccess: refresh,
  })
  return { list, create, remove }
}

/** The boards of a workspace, with create and pin. */
export function useBoards(workspace: MaybeRefOrGetter<string | null>) {
  const list = useQuery({
    queryKey: computed(() => queryKeys.boards(toValue(workspace) ?? '')),
    queryFn: ({ signal }) => boardsResource.list(toValue(workspace) ?? '', signal),
    enabled: computed(() => !!toValue(workspace)),
  })
  const refresh = () => queryClient.invalidateQueries({ predicate: (query) => String(query.queryKey[0]).startsWith('boards.') })
  const create = useMutation({
    mutationFn: (name: string) => boardsResource.create(toValue(workspace) ?? '', name),
    onSuccess: refresh,
  })
  const pin = useMutation({
    mutationFn: (args: { board: string; title: string; state: ExplorationState; exploration?: string }) =>
      boardsResource.pin(args.board, args.title, args.state, args.exploration),
    onSuccess: refresh,
  })
  return { list, create, pin }
}

/** One board, with reorder, rename and delete. */
export function useBoard(board: MaybeRefOrGetter<string>) {
  const query = useQuery({
    queryKey: computed(() => queryKeys.board(toValue(board))),
    queryFn: ({ signal }) => boardsResource.get(toValue(board), signal),
  })
  const refresh = () => queryClient.invalidateQueries({ predicate: (q) => String(q.queryKey[0]).startsWith('boards.') })
  const update = useMutation({
    mutationFn: (args: Omit<BoardsUpdateArgs, 'board'>) => boardsResource.update({ board: toValue(board), ...args }),
    onSuccess: refresh,
  })
  const remove = useMutation({
    mutationFn: () => boardsResource.remove(toValue(board)),
    onSuccess: refresh,
  })
  return { query, update, remove }
}
