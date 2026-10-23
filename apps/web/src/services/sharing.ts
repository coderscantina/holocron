import type { DimensionField, ExplorationState, NamedMeasure, Share, SharingCreateArgs, Site } from '~/generated'
import { EXPLORATION_DIMENSIONS_MAX, EXPLORATION_MEASURES_MAX } from '~/generated/limits'

import { defaultState } from './exploration'

const DAY_MS = 86_400_000
/** Longest range a new share opens with, in days. */
const OPENING_DAYS = 30

export interface ShareDraft {
  name: string
  measures: NamedMeasure[]
  dimensions: DimensionField[]
  rangeMaxDays: number
  narrowing: boolean
  exportGrant: boolean
  recordingsGrant: boolean
  lifetimeDays: number
}

/**
 * The `sharing.create` arguments of a draft: the limits, and an opening view inside them. The
 * view shows the first measures and the first dimension over the last 30 days, or fewer when
 * the limit is shorter; it compares nothing, since a comparison counts toward the limit
 * (decision 0007).
 */
export function shareArgs(site: Site, draft: ShareDraft, now = Date.now()): SharingCreateArgs {
  const measures = draft.measures.slice(0, EXPLORATION_MEASURES_MAX)
  const dimensions = draft.dimensions.slice(0, Math.min(1, EXPLORATION_DIMENSIONS_MAX))
  const first = measures[0]
  const state: ExplorationState = {
    ...defaultState(site.id, site.timezone),
    measures,
    dimensions,
    range: { kind: 'relative', unit: 'day', count: Math.min(OPENING_DAYS, draft.rangeMaxDays) },
    compare: 'none',
    sort: dimensions.length > 0 && first ? [{ by: first, dir: 'desc' }] : [],
    visualization: dimensions.length > 0 ? 'table' : 'bar',
  }
  return {
    name: draft.name,
    target: {
      kind: 'exploration',
      state,
      limits: {
        measures: draft.measures,
        dimensions: draft.dimensions,
        properties: [],
        range_max_days: draft.rangeMaxDays,
        narrowing_allowed: draft.narrowing,
      },
    },
    export: draft.exportGrant,
    recordings: draft.recordingsGrant,
    expires_at: now + draft.lifetimeDays * DAY_MS,
  }
}

export type ShareStatus = 'active' | 'expired' | 'revoked'

export const shareStatus = (share: Share, now = Date.now()): ShareStatus =>
  share.revoked_at ? 'revoked' : share.expires_at <= now ? 'expired' : 'active'
