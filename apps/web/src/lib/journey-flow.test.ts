import { describe, expect, it } from 'vitest'

import { biggestDropOff, emphasis, journeyFlow } from './journey-flow'

const sequences = [
  { steps: ['/'], sessions: 50 },
  { steps: ['/', '/pricing', '/signup'], sessions: 30 },
  { steps: ['/blog', '/pricing'], sessions: 15 },
  { steps: ['/docs', '/'], sessions: 5 },
]

describe('journeyFlow', () => {
  it('ends shorter sequences in an exit and groups values past the cap as other', () => {
    const flow = journeyFlow(sequences, 3, 2)
    expect(flow.route(sequences[0]!)).toEqual(['s0:/', 'x1'])
    expect(flow.route(sequences[1]!)).toEqual(['s0:/', 's1:/pricing', 's2:/signup'])
    expect(flow.route(sequences[3]!)).toEqual(['o0', 's1:/', 'x2'])
    expect(flow.nodes.find((node) => node.id === 'x1')?.sessions).toBe(50)
  })

  it('emphasises the links of the matching sequences only', () => {
    const flow = journeyFlow(sequences, 3, 2)
    const sums = emphasis(flow, sequences, (sequence) => sequence.steps.includes('/pricing'))
    expect(sums.get('s0:/>s1:/pricing')).toBe(30)
    expect(sums.get('s1:/pricing>x2')).toBe(15)
    expect(sums.has('s0:/>x1')).toBe(false)
  })
})

describe('biggestDropOff', () => {
  it('picks the value most sessions ended on, counting reach only where an end can be seen', () => {
    // `/signup` at the last step may go on, so neither its exit nor its reach counts.
    expect(biggestDropOff(sequences, 3)).toEqual({ value: '/', exits: 55, reached: 85 })
  })
})
