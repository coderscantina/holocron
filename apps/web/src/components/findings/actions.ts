import type { RouteLocationRaw } from 'vue-router'

import type { Finding, Group, SiteId } from '~/generated'
import { SUMMARY_STALE_MS } from '~/generated/limits'
import { encodeState } from '~/services/exploration'

export type FindingAction =
  | { id: 'filter'; filter: Group }
  | { id: 'report' | 'details'; route: 'goals' | 'funnels' | 'performance' | 'explore' }
  | { id: 'replays'; count: number; filter: Group }
  | { id: 'annotate' }

/** What the site and the person allow, and what has been measured so far. */
export interface ActionContext {
  /** Replays matching the finding's filter; null while recording is off, unreadable or not counted yet. */
  replays: number | null
  canAnnotate: boolean
}

/** Report routes by rule family; the report opens with the finding's own state. */
const reportRoutes = { goal_pace: 'goals', funnel_step: 'funnels', performance: 'performance' } as const

/** Where an action that opens a report goes: the report with the finding's own state. */
export const reportLocation = (siteId: SiteId, route: Extract<FindingAction, { route: string }>['route'], finding: Finding): RouteLocationRaw => ({
  name: route,
  params: { siteId },
  query: { s: encodeState(finding.state) },
})

/** The report a finding opens: its family's report, or Explore for movers and rules this build does not know. */
export function openFinding(siteId: SiteId, finding: Finding, known: boolean): RouteLocationRaw {
  const route = known && finding.values.kind !== 'dimension_mover' ? reportRoutes[finding.values.kind] : 'explore'
  return reportLocation(siteId, route, finding)
}

/**
 * The filter that applies to the overview and selects replays. Only a dimension mover has one
 * that is a plain value. The other families filter on time windows and report conditions,
 * which belong to their report and not to the traffic on screen.
 */
export function filterOf(finding: Finding, known: boolean): Group | null {
  return known && finding.values.kind === 'dimension_mover' ? finding.state.filter : null
}

/**
 * The actions of a finding in display order; the first is the main one. A replay action needs
 * a count above 0, and an unknown rule only offers its state in Explore.
 */
export function findingActions(finding: Finding, known: boolean, context: ActionContext): FindingAction[] {
  const filter = filterOf(finding, known)
  const actions: FindingAction[] = []
  if (filter) actions.push({ id: 'filter', filter })
  if (known && finding.values.kind !== 'dimension_mover') {
    actions.push({ id: 'report', route: reportRoutes[finding.values.kind] })
  }
  if (!known) actions.push({ id: 'details', route: 'explore' })
  if (filter && context.replays !== null && context.replays > 0) {
    actions.push({ id: 'replays', count: context.replays, filter })
  }
  if (context.canAnnotate) actions.push({ id: 'annotate' })
  return actions
}

export type ListState = 'pending' | 'behind' | 'current'

/**
 * Whether findings can be shown: none exist before the site's first summary, and a summary
 * more than 30 minutes old is too far behind to state what is unusual today.
 */
export function listState(computedAt: number | null, now: number): ListState {
  if (computedAt === null) return 'pending'
  return now - computedAt > SUMMARY_STALE_MS ? 'behind' : 'current'
}
