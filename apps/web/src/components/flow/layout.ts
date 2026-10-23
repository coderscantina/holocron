/**
 * Layout of a flow diagram (Sankey-like): nodes in columns from left to right, each as tall as
 * the larger of what flows in and out of it, and links as bands between them. Plain math, no
 * DOM, so the views and the tests share it.
 */

/** A node in one column; columns count from 0 at the left. */
export interface FlowNode {
  id: string
  column: number
}

/** What flows from one node to a node in a later column. */
export interface FlowLink {
  source: string
  target: string
  value: number
}

/** A node as the flow diagram draws it. */
export interface FlowDiagramNode extends FlowNode {
  label: string
  /** The node's figure after the label: "1,204" or "€3,210.00". */
  figure: string
  /** `accent` marks what matters (the money, a selected end), `quiet` exits and leftovers. */
  tone?: 'accent' | 'quiet'
}

/** A link as the flow diagram draws it. */
export interface FlowDiagramLink extends FlowLink {
  /** The part of `value` drawn in the accent, along the top of the band. */
  emphasis?: number
}

export interface FlowBox {
  width: number
  height: number
  /** Width of a node's bar. */
  nodeWidth: number
  /** Vertical space between two nodes of a column. */
  gap: number
}

export interface PlacedNode<Node extends FlowNode> {
  node: Node
  x: number
  y: number
  height: number
  value: number
}

export interface PlacedLink<Link extends FlowLink> {
  link: Link
  /** Right edge of the source bar and left edge of the target bar. */
  x0: number
  x1: number
  /** Top of the band where it leaves the source and where it enters the target. */
  y0: number
  y1: number
  width: number
}

export interface FlowLayout<Node extends FlowNode, Link extends FlowLink> {
  nodes: PlacedNode<Node>[]
  links: PlacedLink<Link>[]
  /** Pixels per unit of value, the same for every column. */
  scale: number
}

/**
 * Places `nodes` top-down in their given order within each column and fits the fullest column
 * to the box height. Nodes nothing flows through and links to unknown nodes are left out.
 * Bands leave and enter a node ordered by the position of their other end, so they cross as
 * little as the node order allows.
 */
export function layoutFlow<Node extends FlowNode, Link extends FlowLink>(
  nodes: readonly Node[],
  links: readonly Link[],
  box: FlowBox
): FlowLayout<Node, Link> {
  const known = new Set(nodes.map((node) => node.id))
  const kept = links.filter((link) => link.value > 0 && known.has(link.source) && known.has(link.target))
  const into = new Map<string, number>()
  const out = new Map<string, number>()
  for (const link of kept) {
    out.set(link.source, (out.get(link.source) ?? 0) + link.value)
    into.set(link.target, (into.get(link.target) ?? 0) + link.value)
  }

  const valued = nodes
    .map((node) => ({ node, value: Math.max(into.get(node.id) ?? 0, out.get(node.id) ?? 0) }))
    .filter((entry) => entry.value > 0)
  const columns = new Map<number, typeof valued>()
  for (const entry of valued) columns.set(entry.node.column, [...(columns.get(entry.node.column) ?? []), entry])

  const scale = Math.min(
    ...[...columns.values()].map((column) => {
      const total = column.reduce((sum, entry) => sum + entry.value, 0)
      return Math.max(0, box.height - box.gap * (column.length - 1)) / total
    })
  )
  const last = Math.max(0, ...columns.keys())
  const step = last > 0 ? (box.width - box.nodeWidth) / last : 0

  const placed = new Map<string, PlacedNode<Node>>()
  for (const column of columns.values()) {
    let y = 0
    for (const { node, value } of column) {
      const height = value * scale
      placed.set(node.id, { node, x: node.column * step, y, height, value })
      y += height + box.gap
    }
  }

  const bands = kept.flatMap((link) => {
    const source = placed.get(link.source)
    const target = placed.get(link.target)
    return source && target ? [{ link, source, target, width: link.value * scale, y0: 0, y1: 0 }] : []
  })
  const stack = (side: 'source' | 'target', other: 'source' | 'target', end: 'y0' | 'y1') => {
    const ordered = [...bands].sort((a, b) => a[other].y - b[other].y)
    const offsets = new Map<string, number>()
    for (const band of ordered) {
      const at = offsets.get(band[side].node.id) ?? band[side].y
      band[end] = at
      offsets.set(band[side].node.id, at + band.width)
    }
  }
  stack('source', 'target', 'y0')
  stack('target', 'source', 'y1')

  return {
    nodes: [...placed.values()],
    links: bands.map(({ link, source, target, width, y0, y1 }) => ({
      link,
      x0: source.x + box.nodeWidth,
      x1: target.x,
      y0,
      y1,
      width,
    })),
    scale: Number.isFinite(scale) ? scale : 0,
  }
}

/** The outline of a band `width` thick from (x0, y0) to (x1, y1), as an SVG path. */
export function bandPath(x0: number, y0: number, x1: number, y1: number, width: number): string {
  const mid = (x0 + x1) / 2
  const r = (value: number) => Math.round(value * 100) / 100
  return [
    `M${r(x0)},${r(y0)}`,
    `C${r(mid)},${r(y0)} ${r(mid)},${r(y1)} ${r(x1)},${r(y1)}`,
    `L${r(x1)},${r(y1 + width)}`,
    `C${r(mid)},${r(y1 + width)} ${r(mid)},${r(y0 + width)} ${r(x0)},${r(y0 + width)}`,
    'Z',
  ].join('')
}
