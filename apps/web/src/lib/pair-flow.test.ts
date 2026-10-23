import { describe, expect, it } from 'vitest'

import { mainCurrency, pairFlow, sumIn } from './pair-flow'

describe('pairFlow', () => {
  it('keeps the largest values of each side and sends the rest through other', () => {
    const flow = pairFlow(
      [
        { from: 'search', to: 'spring', value: 10 },
        { from: 'social', to: 'spring', value: 4 },
        { from: 'email', to: 'autumn', value: 2 },
        { from: 'social', to: 'idle', value: 0 },
      ],
      { from: 2, to: 2 }
    )
    expect(flow.nodes.map((node) => [node.id, node.value])).toEqual([
      ['from:search', 10],
      ['from:social', 4],
      ['from*', 2],
      ['to:spring', 14],
      ['to:autumn', 2],
    ])
    expect(flow.links).toContainEqual({ source: 'from*', target: 'to:autumn', value: 2 })
  })
})

describe('money', () => {
  const eur = (minor: number) => ({ currency: 'EUR', exponent: 2, minor_units: minor, amount: (minor / 100).toFixed(2) })

  it('sums one currency exactly and picks the largest currency', () => {
    expect(sumIn([eur(1999), eur(1), eur(-5), { currency: 'JPY', exponent: 0, minor_units: 300, amount: '300' }], 'EUR')).toEqual(eur(1995))
    expect(sumIn([eur(-5)], 'EUR')?.amount).toBe('-0.05')
    expect(mainCurrency([eur(500), { currency: 'USD', exponent: 2, minor_units: 900, amount: '9.00' }])).toBe('USD')
  })
})
