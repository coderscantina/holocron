import type { GoalDefinition, GoalId, SiteId } from '~/generated'

import { api } from '..'

/** Saved, versioned goals of one site (0003 Goals). */
export const goalsResource = {
  list: (site: SiteId, signal?: AbortSignal) => api.query('goals.list', { site }, { signal }),
  get: (site: SiteId, goal: GoalId, version?: number, signal?: AbortSignal) =>
    api.query('goals.get', { site, goal, version }, { signal }),
  save: (site: SiteId, name: string, definition: GoalDefinition, goal?: GoalId) =>
    api.command('goals.save', { site, name, definition, goal }),
  archive: (site: SiteId, goal: GoalId) => api.command('goals.archive', { site, goal }),
}
