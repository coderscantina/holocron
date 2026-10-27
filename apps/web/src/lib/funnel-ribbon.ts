import type { FunnelLeak, Range, ResolvedRange } from '~/generated'

/**
 * Layout of the funnel ribbon: one column per step, as tall as the share of step 1 that reached
 * it, joined by the band of those who went on. Those who dropped peel off the band's bottom edge
 * and curve down into one flow per destination (left, back, a next page, other), stacked under
 * the ribbon. All in SVG user units; the view scales the drawing to its box.
 */

/** Where a flow of leavers went next. */
export type LeakKind = 'exited' | 'back' | 'page' | 'other'

export type RibbonFlow = { kind: LeakKind; path: string | null; identities: number; d: string; labelX: number; labelY: number }

export type RibbonStep = { x: number; y: number; width: number; height: number; reached: number; share: number }

export type RibbonLeak = {
  /** The step the leavers reached last, 0-based. */
  step: number
  dropped: number
  /** Dropped out of those who reached the step. */
  rate: number
  flows: RibbonFlow[]
}

export type Ribbon = { width: number; height: number; steps: RibbonStep[]; bands: string[]; leaks: RibbonLeak[] }

/** `top` is the room above the columns for their labels. */
export type RibbonOptions = { width: number; height: number; column?: number; gap?: number; minFlow?: number; top?: number }

const TOP = 52
/** Room under the ribbon before the leak flows start. */
const LEAK_MARGIN = 28

/** The flows of a leak, biggest destination kinds first: left, back, named pages, the rest. */
export function flowsOf(leak: FunnelLeak | undefined, dropped: number): Omit<RibbonFlow, 'd' | 'labelX' | 'labelY'>[] {
  if (!leak) return dropped > 0 ? [{ kind: 'other', path: null, identities: dropped }] : []
  return [
    { kind: 'exited' as const, path: null, identities: leak.exited },
    { kind: 'back' as const, path: null, identities: leak.back },
    ...leak.next.map((page) => ({ kind: 'page' as const, path: page.path, identities: page.identities })),
    { kind: 'other' as const, path: null, identities: leak.other },
  ].filter((flow) => flow.identities > 0)
}

/** A band from a vertical segment at `x0` to one at `x1`, bending smoothly between. */
function band(x0: number, top0: number, bottom0: number, x1: number, top1: number, bottom1: number): string {
  const mid = (x0 + x1) / 2
  return `M${x0},${top0} C${mid},${top0} ${mid},${top1} ${x1},${top1} L${x1},${bottom1} C${mid},${bottom1} ${mid},${bottom0} ${x0},${bottom0} Z`
}

export function ribbon(reached: readonly number[], leaks: readonly FunnelLeak[], options: RibbonOptions): Ribbon {
  const { width, height, column = 10, gap = 22, minFlow = 2, top: headroom = TOP } = options
  const first = reached[0] ?? 0
  const count = reached.length
  const pitch = count > 1 ? (width - column) / (count - 1) : 0
  const flowsPerStep = reached.slice(0, -1).map((n, i) => flowsOf(leaks[i], n - (reached[i + 1] ?? 0)))
  // The ribbon and the tallest stack of leak flows share the height below the step labels.
  const deepest = Math.max(0, ...reached.slice(0, -1).map((n, i) => (first > 0 ? (n - reached[i + 1]!) / first : 0)))
  const spacing = Math.max(0, ...flowsPerStep.map((flows) => flows.length - 1)) * gap
  const scale = Math.max(0, (height - headroom - LEAK_MARGIN - spacing - minFlow) / (1 + deepest))
  const tall = (n: number) => (first > 0 ? (n / first) * scale : 0)

  const steps: RibbonStep[] = reached.map((n, i) => ({
    x: i * pitch,
    y: headroom,
    width: column,
    height: Math.max(n > 0 ? minFlow : 0, tall(n)),
    reached: n,
    share: first > 0 ? n / first : 0,
  }))
  const bands = steps.slice(1).map((next, i) => {
    const from = steps[i]!
    return band(from.x + column, headroom, headroom + next.height, next.x, headroom, headroom + next.height)
  })
  const peeled = flowsPerStep.map((flows, i): RibbonLeak => {
    const from = steps[i]!
    const kept = steps[i + 1]!.height
    const dropped = from.reached - steps[i + 1]!.reached
    let source = headroom + kept
    let target = headroom + scale + LEAK_MARGIN
    const x0 = from.x + column
    const x1 = from.x + column + Math.min(pitch * 0.45, 160)
    return {
      step: i,
      dropped,
      rate: from.reached > 0 ? dropped / from.reached : 0,
      flows: flows.map((flow) => {
        const thick = Math.max(minFlow, tall(flow.identities))
        const d = band(x0, source, source + thick, x1, target, target + thick)
        const placed = { ...flow, d, labelX: x1 + 6, labelY: target + thick / 2 }
        source += thick
        target += thick + gap
        return placed
      }),
    }
  })
  return { width, height, steps, bands, leaks: peeled }
}

const DAY_MS = 86_400_000
const localDate = (ms: number, timeZone: string) =>
  new Intl.DateTimeFormat('en-CA', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(ms)
const shiftDate = (date: string, days: number) => {
  const shifted = new Date(`${date}T00:00:00Z`)
  shifted.setUTCDate(shifted.getUTCDate() + days)
  return shifted.toISOString().slice(0, 10)
}

/** As many local days as the resolved range spans, right before it: a funnel's previous period. */
export function previousDays({ start, end, timezone }: ResolvedRange): Range {
  const first = localDate(start, timezone)
  const days = Math.max(1, Math.round((end - start) / DAY_MS))
  return { kind: 'days', start: shiftDate(first, -days), end: shiftDate(first, -1) }
}

/** The step whose leavers are the largest share of those who reached it; `null` without drop. */
export function worstLeak(leaks: readonly RibbonLeak[]): RibbonLeak | null {
  return leaks.reduce<RibbonLeak | null>((worst, leak) => (leak.dropped > 0 && (!worst || leak.rate > worst.rate) ? leak : worst), null)
}
