import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SUMMARY_SITES_MAX } from '~/generated'

const query = vi.hoisted(() => vi.fn())
vi.mock('..', () => ({ api: { query } }))

import { chunkSites, summariesResource } from './summaries'

const ids = (count: number) => Array.from({ length: count }, (_, index) => `site-${index}`)

describe('chunkSites', () => {
  it('splits at the server limit', () => {
    expect(chunkSites(ids(SUMMARY_SITES_MAX)).map((chunk) => chunk.length)).toEqual([SUMMARY_SITES_MAX])
    expect(chunkSites(ids(SUMMARY_SITES_MAX + 1)).map((chunk) => chunk.length)).toEqual([SUMMARY_SITES_MAX, 1])
  })
})

describe('summariesResource.forSites', () => {
  beforeEach(() => {
    query.mockReset()
  })

  it('merges the answers of every chunk', async () => {
    query.mockImplementation(async (_name: string, args: { sites: string[] }) => ({
      summaries: args.sites.map((site) => ({ site })),
    }))
    const all = await summariesResource.forSites(ids(250))
    expect(query).toHaveBeenCalledTimes(3)
    expect(all).toHaveLength(250)
  })
})
