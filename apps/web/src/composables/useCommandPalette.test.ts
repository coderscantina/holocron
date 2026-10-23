import { describe, expect, it } from 'vitest'

import { GROUP_LIMIT, visibleResults } from './useCommandPalette'

const items = Array.from({ length: 12 }, (_, i) => ({ id: String(i), label: i < 10 ? `Campaign ${i}` : `Other ${i}` }))

describe('visibleResults', () => {
  it('caps a group until it is expanded', () => {
    expect(visibleResults(items, '', false).shown).toHaveLength(GROUP_LIMIT)
    expect(visibleResults(items, '', true).shown).toHaveLength(12)
  })

  it('searches the whole group before capping and reports the total', () => {
    const result = visibleResults(items, ' campaign', false)
    expect(result.total).toBe(10)
    expect(result.shown.map((item) => item.id)).toEqual(items.slice(0, GROUP_LIMIT).map((item) => item.id))
    expect(visibleResults(items, 'other 11', false).shown).toEqual([items[11]])
  })
})
