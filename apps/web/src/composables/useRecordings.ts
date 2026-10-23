import { keepPreviousData, useInfiniteQuery, useMutation, useQuery } from '@tanstack/vue-query'
import { computed, toValue, type MaybeRefOrGetter } from 'vue'

import { recordingsResource } from '~/api/resources/recordings'
import { shareKey, type ShareScope } from '~/api/resources/sharing'
import type {
  CaptureArgs,
  HeatmapCellsArgs,
  HeatmapPagesArgs,
  RecordingPolicyUpdateArgs,
  RecordingsListArgs,
} from '~/generated'
import { queryClient } from '~/plugins/vue-query'

const DAY_MS = 86_400_000
const MINUTE_MS = 60_000

/** Ranges the recording screens offer. Replays are kept 14 days, heatmaps 90. */
export const replayRanges = { '1d': 1, '7d': 7, '14d': 14 } as const
export const heatmapRanges = { '7d': 7, '30d': 30, '90d': 90 } as const
export type ReplayRange = keyof typeof replayRanges
export type HeatmapRange = keyof typeof heatmapRanges
export const isReplayRange = (value: string): value is ReplayRange => value in replayRanges
export const isHeatmapRange = (value: string): value is HeatmapRange => value in heatmapRanges

/**
 * The last `days` days up to the next full minute. Rounding keeps the query key stable while a
 * screen stays open, so the cache is reused instead of refetched on every render.
 */
export function lastDays(days: number, now = Date.now()) {
  const end = Math.ceil(now / MINUTE_MS) * MINUTE_MS
  return { start: end - days * DAY_MS, end }
}

const keys = {
  policy: (site: string) => ['recordings.policy', site] as const,
  budget: (site: string) => ['recordings.budget', site] as const,
  list: (args: Omit<RecordingsListArgs, 'after'>) => ['recordings.list', args] as const,
  replay: (site: string, capture: string) => ['recordings.get', site, capture] as const,
  heatmapPages: (args: HeatmapPagesArgs) => ['heatmaps.pages', args] as const,
  heatmapCells: (args: HeatmapCellsArgs) => ['heatmaps.cells', args] as const,
}

export function useRecordingPolicy(site: MaybeRefOrGetter<string>) {
  return useQuery({
    queryKey: computed(() => keys.policy(toValue(site))),
    queryFn: ({ signal }) => recordingsResource.policy(toValue(site), signal),
  })
}

export function useUpdateRecordingPolicy() {
  return useMutation({
    mutationFn: ({ args, idempotencyKey }: { args: RecordingPolicyUpdateArgs; idempotencyKey: string }) =>
      recordingsResource.updatePolicy(args, idempotencyKey),
    onSuccess: (_, { args }) => queryClient.invalidateQueries({ queryKey: keys.policy(args.site) }),
  })
}

export function useRecordingBudget(site: MaybeRefOrGetter<string>) {
  return useQuery({
    queryKey: computed(() => keys.budget(toValue(site))),
    queryFn: ({ signal }) => recordingsResource.budget(toValue(site), signal),
    staleTime: MINUTE_MS,
  })
}

/**
 * Replay list pages, newest first; `fetchNextPage` continues from the server's cursor. With a
 * share scope the list reads as the share viewer.
 */
export function useReplays(args: MaybeRefOrGetter<Omit<RecordingsListArgs, 'after'>>, share?: ShareScope) {
  return useInfiniteQuery({
    queryKey: computed(() => shareKey(share, keys.list(toValue(args)))),
    queryFn: ({ pageParam, signal }) =>
      recordingsResource.list({ ...toValue(args), after: pageParam }, signal, share?.client),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next,
    placeholderData: keepPreviousData,
    staleTime: MINUTE_MS,
  })
}

/** Every playback page of one replay, in order; the player asks for the next one as it goes. */
export function useReplay(site: MaybeRefOrGetter<string>, capture: MaybeRefOrGetter<string>, share?: ShareScope) {
  return useInfiniteQuery({
    queryKey: computed(() => shareKey(share, keys.replay(toValue(site), toValue(capture)))),
    queryFn: ({ pageParam, signal }) =>
      recordingsResource.get({ site: toValue(site), capture: toValue(capture), after: pageParam }, signal, share?.client),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next,
  })
}

/** Saves or unsaves a replay; lists and the replay are read again for the new state. */
export function useSaveReplay() {
  return useMutation({
    mutationFn: ({ args, save }: { args: CaptureArgs; save: boolean }) =>
      save ? recordingsResource.save(args) : recordingsResource.unsave(args),
    onSuccess: (_, { args }) =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: ['recordings.list'] }),
        queryClient.invalidateQueries({ queryKey: keys.replay(args.site, args.capture) }),
        queryClient.invalidateQueries({ queryKey: keys.budget(args.site) }),
      ]),
  })
}

export function useHeatmapPages(args: MaybeRefOrGetter<HeatmapPagesArgs>) {
  return useQuery({
    queryKey: computed(() => keys.heatmapPages(toValue(args))),
    queryFn: ({ signal }) => recordingsResource.heatmapPages(toValue(args), signal),
    placeholderData: keepPreviousData,
    staleTime: MINUTE_MS,
  })
}

export function useHeatmapCells(args: MaybeRefOrGetter<HeatmapCellsArgs | null>) {
  return useQuery({
    queryKey: computed(() => keys.heatmapCells(toValue(args) ?? { site: '', path: '', start: 0, end: 0, viewport: 'desktop' })),
    queryFn: ({ signal }) => {
      const value = toValue(args)
      if (!value) throw new Error('no page selected')
      return recordingsResource.heatmapCells(value, signal)
    },
    enabled: computed(() => toValue(args) !== null),
    placeholderData: keepPreviousData,
    staleTime: MINUTE_MS,
  })
}
