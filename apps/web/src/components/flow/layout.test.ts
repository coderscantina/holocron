import { describe, expect, it } from 'vitest'

import { bandPath, layoutFlow, type FlowLink, type FlowNode } from './layout'

const box = { width: 210, height: 100, nodeWidth: 10, gap: 10 }
const node = (id: string, column: number): FlowNode => ({ id, column })
const link = (source: string, target: string, value: number): FlowLink => ({ source, target, value })

describe('layoutFlow', () => {
  it('fits the fullest column to the height and spreads columns over the width', () => {
    const layout = layoutFlow(
      [node('a', 0), node('b', 0), node('x', 1), node('y', 1), node('z', 1)],
      [link('a', 'x', 30), link('a', 'y', 10), link('b', 'z', 30)],
      box
    )
    // Column 1 has three nodes, two gaps: (100 - 20) / 70 units.
    expect(layout.scale).toBeCloseTo(80 / 70)
    const at = Object.fromEntries(layout.nodes.map((placed) => [placed.node.id, placed]))
    expect(at.a?.x).toBe(0)
    expect(at.x?.x).toBe(200)
    expect(at.a?.height).toBeCloseTo(40 * layout.scale)
    expect(at.b?.y).toBeCloseTo(40 * layout.scale + 10)
    expect(at.z?.y).toBeCloseTo(40 * layout.scale + 20)
  })

  it('sizes a node by the larger of its inflow and outflow', () => {
    const layout = layoutFlow([node('a', 0), node('b', 1), node('c', 2)], [link('a', 'b', 10), link('b', 'c', 4)], box)
    expect(layout.nodes.find((placed) => placed.node.id === 'b')?.value).toBe(10)
  })

  it('leaves out nodes nothing flows through and links to unknown nodes', () => {
    const layout = layoutFlow([node('a', 0), node('b', 1), node('idle', 1)], [link('a', 'b', 5), link('a', 'gone', 5)], box)
    expect(layout.nodes.map((placed) => placed.node.id)).toEqual(['a', 'b'])
    expect(layout.links).toHaveLength(1)
  })

  it('stacks bands at each end in the order of their other end', () => {
    const layout = layoutFlow(
      [node('a', 0), node('b', 0), node('x', 1), node('y', 1)],
      [link('a', 'y', 20), link('a', 'x', 20), link('b', 'x', 20)],
      { ...box, gap: 0 }
    )
    const band = (source: string, target: string) =>
      layout.links.find((placed) => placed.link.source === source && placed.link.target === target)
    expect(band('a', 'x')?.y0).toBe(0)
    expect(band('a', 'y')?.y0).toBeCloseTo(20 * layout.scale)
    expect(band('a', 'x')?.y1).toBe(0)
    expect(band('b', 'x')?.y1).toBeCloseTo(20 * layout.scale)
    expect(band('a', 'x')?.x0).toBe(10)
    expect(band('a', 'x')?.x1).toBe(200)
  })
})

describe('bandPath', () => {
  it('outlines a band with curved top and bottom edges', () => {
    expect(bandPath(0, 0, 100, 50, 10)).toBe('M0,0C50,0 50,50 100,50L100,60C50,60 50,10 0,10Z')
  })
})
