import type { FlowLink, FlowNode } from '~/components/flow/layout'
import type { JourneySequence } from '~/generated'

/**
 * Journey sequences as a flow: one column per step, the most common values of each step as
 * nodes, the rest of a step as one "other" node, and an exit node after every step where
 * sessions ended. A sequence as long as the report's length may go on, so it has no exit
 * (0003 Journeys, terminal).
 */

export type JourneyNode = FlowNode &
  ({ kind: 'step'; value: string } | { kind: 'other' } | { kind: 'exit' }) & { sessions: number }

export interface JourneyFlow {
  nodes: JourneyNode[]
  links: FlowLink[]
  /** The node IDs a sequence passes, its exit included. */
  route: (sequence: JourneySequence) => string[]
}

const stepId = (column: number, value: string) => `s${column}:${value}`
const otherId = (column: number) => `o${column}`
const exitId = (column: number) => `x${column}`

export function journeyFlow(sequences: readonly JourneySequence[], length: number, perColumn: number): JourneyFlow {
  const kept = Array.from({ length }, (_, column) => {
    const tally = new Map<string, number>()
    for (const sequence of sequences) {
      const value = sequence.steps[column]
      if (value !== undefined) tally.set(value, (tally.get(value) ?? 0) + sequence.sessions)
    }
    return [...tally].sort(([a, x], [b, y]) => y - x || (a < b ? -1 : 1)).slice(0, perColumn)
  })
  const keptIds = new Set(kept.flatMap((values, column) => values.map(([value]) => stepId(column, value))))

  const route = (sequence: JourneySequence) => {
    const ids = sequence.steps.slice(0, length).map((value, column) => {
      const id = stepId(column, value)
      return keptIds.has(id) ? id : otherId(column)
    })
    return sequence.steps.length < length ? [...ids, exitId(sequence.steps.length)] : ids
  }

  const totals = new Map<string, number>()
  const flows = new Map<string, FlowLink>()
  for (const sequence of sequences) {
    const ids = route(sequence)
    ids.forEach((id, index) => {
      totals.set(id, (totals.get(id) ?? 0) + sequence.sessions)
      const next = ids[index + 1]
      if (next === undefined) return
      const key = `${id}>${next}`
      const flow = flows.get(key) ?? { source: id, target: next, value: 0 }
      flow.value += sequence.sessions
      flows.set(key, flow)
    })
  }

  const nodes = Array.from({ length }, (_, column): JourneyNode[] => [
    ...(kept[column] ?? []).map(([value, sessions]): JourneyNode => ({ id: stepId(column, value), column, kind: 'step', value, sessions })),
    { id: otherId(column), column, kind: 'other', sessions: totals.get(otherId(column)) ?? 0 },
    { id: exitId(column), column, kind: 'exit', sessions: totals.get(exitId(column)) ?? 0 },
  ]).flatMap((column) => column.filter((node) => node.sessions > 0))

  return { nodes, links: [...flows.values()], route }
}

/** Per link, the sessions of the sequences `matches` picks, keyed `source>target`. */
export function emphasis(
  flow: JourneyFlow,
  sequences: readonly JourneySequence[],
  matches: (sequence: JourneySequence, route: string[]) => boolean
): Map<string, number> {
  const sums = new Map<string, number>()
  for (const sequence of sequences) {
    const ids = flow.route(sequence)
    if (!matches(sequence, ids)) continue
    for (let index = 1; index < ids.length; index++) {
      const key = `${ids[index - 1]}>${ids[index]}`
      sums.set(key, (sums.get(key) ?? 0) + sequence.sessions)
    }
  }
  return sums
}

export interface DropOff {
  value: string
  /** Sessions that ended on the value. */
  exits: number
  /** Sessions that reached the value where an end could be seen: before the last step. */
  reached: number
}

/** The value most sessions ended on, with how many reached it; null when none ended early. */
export function biggestDropOff(sequences: readonly JourneySequence[], length: number): DropOff | null {
  const byValue = new Map<string, DropOff>()
  for (const sequence of sequences) {
    const ended = sequence.steps.length < length
    sequence.steps.forEach((value, index) => {
      if (index >= length - 1 && !ended) return
      const entry = byValue.get(value) ?? { value, exits: 0, reached: 0 }
      entry.reached += sequence.sessions
      if (ended && index === sequence.steps.length - 1) entry.exits += sequence.sessions
      byValue.set(value, entry)
    })
  }
  return [...byValue.values()].reduce<DropOff | null>((top, entry) => (entry.exits > (top?.exits ?? 0) ? entry : top), null)
}
