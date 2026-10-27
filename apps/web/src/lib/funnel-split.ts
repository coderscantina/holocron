import type { Cell, FunnelResult, Metric } from '~/generated'

/**
 * The rows of the table under a split funnel: "All traffic" first, then the groups by visitors
 * entered, as the server returns them. Ten show until the person expands the table.
 */

export const GROUPS_SHOWN = 10

export type SplitReach = {
  /** Share of the group's step-1 visitors who reached the step. */
  share: number
  /** The same share for all traffic, where the marker sits. */
  overall: number
  /** The largest share any listed row has at this step; the bar's full width. */
  top: number
}

export type SplitRow = {
  key: string
  /** The group's value; `null` for "All traffic". */
  cell: Cell | null
  entered: number
  /** Visitors who reached each step, the first included. */
  steps: number[]
  /** One per step after the first. */
  reach: SplitReach[]
  conversion: Metric
  /** Median of step 1 to the last step. */
  median: Metric
}

export type Split = { rows: SplitRow[]; hidden: number }

const share = (reached: number | undefined, entered: number) => (entered > 0 ? (reached ?? 0) / entered : 0)

export function splitRows(result: FunnelResult, showAll: boolean): Split {
  const overall = result.steps.map((step) => step.reached)
  const entered = overall[0] ?? 0
  const groups = showAll ? result.groups : result.groups.slice(0, GROUPS_SHOWN)
  const sources = [
    { key: 'all', cell: null, steps: overall, conversion: result.conversion, durations: result.durations },
    ...groups.map((group, index) => ({
      key: `${index}`,
      cell: group.value,
      steps: group.steps.map((step) => step.reached),
      conversion: group.conversion,
      durations: group.durations,
    })),
  ]
  const top = overall.slice(1).map((_, k) => Math.max(...sources.map((row) => share(row.steps[k + 1], row.steps[0] ?? 0))))
  const rows = sources.map(
    (row): SplitRow => ({
      key: row.key,
      cell: row.cell,
      entered: row.steps[0] ?? 0,
      steps: row.steps,
      reach: top.map((max, k) => ({
        share: share(row.steps[k + 1], row.steps[0] ?? 0),
        overall: share(overall[k + 1], entered),
        top: max,
      })),
      conversion: row.conversion,
      median: row.durations.at(-1)?.median ?? { t: 'unavailable', reason: 'no_values' },
    })
  )
  return { rows, hidden: result.groups.length - groups.length }
}
