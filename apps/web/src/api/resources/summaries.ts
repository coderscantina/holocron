import { SUMMARY_SITES_MAX, type FindingList, type SiteId, type Summary } from '~/generated'

import { api } from '..'

/** Splits ids into calls of at most `SUMMARY_SITES_MAX`; the server refuses longer lists. */
export function chunkSites(sites: readonly SiteId[]): SiteId[][] {
  const chunks: SiteId[][] = []
  for (let start = 0; start < sites.length; start += SUMMARY_SITES_MAX) {
    chunks.push(sites.slice(start, start + SUMMARY_SITES_MAX))
  }
  return chunks
}

export const summariesResource = {
  /** Site summaries the caller may read; unauthorized sites are left out without a hint. */
  forSites: async (sites: readonly SiteId[], signal?: AbortSignal): Promise<Summary[]> => {
    const answers = await Promise.all(
      chunkSites(sites).map((chunk) => api.query('summaries.forSites', { sites: chunk }, { signal }))
    )
    return answers.flatMap((answer) => answer.summaries)
  },
  /** The site's findings in rank order, as of its last summary. */
  findings: (site: SiteId, limit: number, signal?: AbortSignal): Promise<FindingList> =>
    api.query('summaries.findings', { site, limit }, { signal }),
}
