import type { ExplorationState, Measure } from '~/generated'

/** Routes that show a whole exploration state from the `s` query parameter. */
export type StatePage =
  | 'explore'
  | 'retention'
  | 'revenue'
  | 'performance'
  | 'funnels'
  | 'journeys'
  | 'attribution'
  | 'goals'
  | 'sessions'

const revenue: Measure[] = ['revenue_net', 'revenue_gross', 'revenue_refunds', 'revenue_count']
const percentiles: Measure[] = ['p50', 'p75', 'p95']

/** The page that draws a state: its report's page, the revenue or performance page, else Explore. */
export function pageOf(state: ExplorationState): StatePage {
  switch (state.report?.kind) {
    case 'retention':
      return 'retention'
    case 'funnel':
      return 'funnels'
    case 'journey':
      return 'journeys'
    case 'attribution':
      return 'attribution'
    case 'goals':
      return 'goals'
    case 'session_list':
      return 'sessions'
    case undefined:
      break
  }
  const only = (allowed: Measure[]) =>
    state.measures.length > 0 && state.measures.every((m) => typeof m === 'string' && allowed.includes(m))
  if (only(percentiles)) return 'performance'
  if (only(revenue) && state.dimensions[0] === 'currency') return 'revenue'
  return 'explore'
}
