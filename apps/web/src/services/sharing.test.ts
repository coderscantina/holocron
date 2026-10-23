import { describe, expect, it } from 'vitest'

import type { Share, Site } from '~/generated'

import { shareArgs, shareStatus } from './sharing'

const site: Site = {
  id: 's1',
  workspace: 'w1',
  name: 'Roadhouse',
  domain: 'roadhouse.test',
  timezone: 'Europe/Vienna',
  session_window_minutes: 30,
  session_policy_version: 1,
  retention: '1y',
  capabilities: ['site.read', 'site.share'],
}

const draft = {
  name: 'Partners',
  measures: ['pageviews', 'visitor_days'] as const,
  dimensions: ['path', 'country'] as const,
  rangeMaxDays: 14,
  narrowing: false,
  exportGrant: true,
  recordingsGrant: false,
  lifetimeDays: 7,
}

describe('share drafts', () => {
  it('open inside their own limits: range within the limit, no comparison, one dimension', () => {
    const args = shareArgs(site, { ...draft, measures: [...draft.measures], dimensions: [...draft.dimensions] }, 0)
    expect(args.expires_at).toBe(7 * 86_400_000)
    expect(args.export).toBe(true)
    if (args.target.kind !== 'exploration') throw new Error('exploration share expected')
    expect(args.target.limits).toEqual({
      measures: ['pageviews', 'visitor_days'],
      dimensions: ['path', 'country'],
      properties: [],
      range_max_days: 14,
      narrowing_allowed: false,
    })
    expect(args.target.state.range).toEqual({ kind: 'relative', unit: 'day', count: 14 })
    expect(args.target.state.compare).toBe('none')
    expect(args.target.state.dimensions).toEqual(['path'])
    expect(args.target.state.site).toBe('s1')
  })

  it('tell active, expired and revoked shares apart', () => {
    const share: Share = {
      id: 'sh1',
      name: 'x',
      target: { kind: 'board', board: 'b1', sites: [] },
      export: false,
      recordings: false,
      expires_at: 100,
      prefix: 'abc',
      created_at: 0,
    }
    expect(shareStatus(share, 50)).toBe('active')
    expect(shareStatus(share, 100)).toBe('expired')
    expect(shareStatus({ ...share, revoked_at: 10 }, 50)).toBe('revoked')
  })
})
