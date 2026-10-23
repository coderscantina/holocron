import type { FlowLink, FlowNode } from '~/components/flow/layout'
import type { Money } from '~/generated'

/**
 * Two-column flows, from where visitors come from to what they became: channel to campaign,
 * touch to conversion. Each side keeps its largest values, the rest of a side flows through
 * one "other" node. Plus exact sums of money per currency for the figures beside the flow.
 */

export interface Pair {
  from: string
  to: string
  value: number
}

export type PairNode = FlowNode & { side: 'from' | 'to'; key: string | null; value: number }

const id = (side: 'from' | 'to', key: string | null) => (key === null ? `${side}*` : `${side}:${key}`)

/** Nodes in descending value, "other" last; `key` is null for "other". */
export function pairFlow(pairs: readonly Pair[], keep: { from: number; to: number }): { nodes: PairNode[]; links: FlowLink[] } {
  const ranked = (side: 'from' | 'to') => {
    const totals = new Map<string, number>()
    for (const pair of pairs) totals.set(pair[side], (totals.get(pair[side]) ?? 0) + pair.value)
    return [...totals].filter(([, value]) => value > 0).sort(([a, x], [b, y]) => y - x || (a < b ? -1 : 1))
  }
  const sides = { from: ranked('from'), to: ranked('to') }
  const kept = { from: new Set(sides.from.slice(0, keep.from).map(([key]) => key)), to: new Set(sides.to.slice(0, keep.to).map(([key]) => key)) }
  const nodeOf = (side: 'from' | 'to', key: string) => id(side, kept[side].has(key) ? key : null)

  const links = new Map<string, FlowLink>()
  for (const pair of pairs) {
    if (pair.value <= 0) continue
    const source = nodeOf('from', pair.from)
    const target = nodeOf('to', pair.to)
    const link = links.get(`${source}>${target}`) ?? { source, target, value: 0 }
    link.value += pair.value
    links.set(`${source}>${target}`, link)
  }

  const nodes = (['from', 'to'] as const).flatMap((side, column): PairNode[] => {
    const all = sides[side]
    const rest = all.slice(keep[side]).reduce((sum, [, value]) => sum + value, 0)
    return [
      ...all.slice(0, keep[side]).map(([key, value]): PairNode => ({ id: id(side, key), column, side, key, value })),
      ...(rest > 0 ? [{ id: id(side, null), column, side, key: null, value: rest }] : []),
    ]
  })
  return { nodes, links: [...links.values()] }
}

/** The currency with the largest amount, as the one a flow is drawn in; null without money. */
export function mainCurrency(amounts: readonly Money[]): string | null {
  return amounts.reduce<Money | null>((top, money) => (!top || Number(money.amount) > Number(top.amount) ? money : top), null)?.currency ?? null
}

/** The amount in `currency` as a number, for drawing; 0 when there is none. */
export const amountIn = (amounts: readonly Money[], currency: string | null): number =>
  Number(amounts.find((money) => money.currency === currency)?.amount ?? 0)

/** The exact sum of the amounts in `currency`, or null when none is in it. */
export function sumIn(amounts: readonly Money[], currency: string): Money | null {
  const matching = amounts.filter((money) => money.currency === currency)
  const first = matching[0]
  if (!first) return null
  const units = matching.reduce((sum, money) => sum + BigInt(money.minor_units), 0n)
  const sign = units < 0n ? '-' : ''
  const digits = (units < 0n ? -units : units).toString().padStart(first.exponent + 1, '0')
  const whole = digits.slice(0, digits.length - first.exponent)
  const fraction = first.exponent > 0 ? `.${digits.slice(digits.length - first.exponent)}` : ''
  const minor = units >= BigInt(Number.MIN_SAFE_INTEGER) && units <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(units) : units.toString()
  return { currency, exponent: first.exponent, minor_units: minor, amount: `${sign}${whole}${fraction}` }
}
