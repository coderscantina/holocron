import { describe, expect, it } from 'vitest'

import { operatorsOf } from './useFilterFields'

describe('operatorsOf', () => {
  it('offers matches and not_matches on glob fields and string properties only', () => {
    expect(operatorsOf({ field: 'path', kind: 'text' })).toContain('matches')
    expect(operatorsOf({ field: 'entry_path', kind: 'text' })).toContain('matches')
    expect(operatorsOf({ field: 'referrer_path', kind: 'text' })).toContain('not_matches')
    expect(operatorsOf({ field: { scope: 'event', key: 'plan' }, kind: 'string' })).toContain('matches')
    expect(operatorsOf({ field: 'country', kind: 'text' })).not.toContain('matches')
    expect(operatorsOf({ field: 'hour', kind: 'integer' })).not.toContain('matches')
    expect(operatorsOf({ field: { scope: 'event', key: 'total' }, kind: 'number' })).not.toContain('not_matches')
  })
})
