import { TOP_SIZE, type DayCounts } from './lib.ts'

/** The query string limit of the protocol envelope (decision 0009). */
const MAX_QUERY_BYTES = 8 * 1024

export interface SiteInfo {
  id: string
  timezone: string
  name: string
}

export class HolocronError extends Error {
  constructor(
    message: string,
    /** `connection` for network and sign-in problems, `refused` for an answer about the request. */
    readonly kind: 'connection' | 'refused',
  ) {
    super(message)
  }
}

interface Metric {
  t: string
  v?: number
}
interface Cell {
  t: string
  v?: string
}
interface Cut {
  rows: { key: Cell[]; values: Metric[] }[]
  series?: { points: { local: string; values: Metric[] }[] }
}
interface ExploreResult {
  main: Cut
  panels: Cut[]
}

const count = (m: Metric | undefined): number => (m?.t === 'count' && typeof m.v === 'number' ? m.v : 0)

export class HolocronApi {
  constructor(
    private readonly base: URL,
    private readonly key: string,
  ) {}

  private async query<T>(name: string, params: [string, string][]): Promise<T> {
    const url = new URL(`api/queries/${name}`, this.base.toString().replace(/\/?$/, '/'))
    for (const [k, v] of params) url.searchParams.append(k, v)
    if (new TextEncoder().encode(url.search).length > MAX_QUERY_BYTES) {
      throw new HolocronError(`${name}: query string over 8 KiB`, 'refused')
    }
    let response: Response
    try {
      response = await fetch(url, {
        headers: { Authorization: `Bearer ${this.key}`, Accept: 'application/json' },
        signal: AbortSignal.timeout(35_000),
      })
    } catch (error) {
      throw new HolocronError(`cannot reach ${this.base.origin}: ${(error as Error).message}`, 'connection')
    }
    if (!response.ok) {
      const body = (await response.json().catch(() => null)) as { error_code?: string; message?: string } | null
      const detail = body?.error_code ? `${body.error_code}: ${body.message ?? ''}` : response.statusText
      const kind = response.status === 401 || response.status === 403 || response.status >= 500 ? 'connection' : 'refused'
      throw new HolocronError(`${name} answered ${response.status} ${detail}`, kind)
    }
    return (await response.json()) as T
  }

  async sites(): Promise<string[]> {
    const { sites } = await this.query<{ sites: { id: string }[] }>('sites.list', [])
    return sites.map((s) => s.id.toLowerCase())
  }

  async site(id: string): Promise<SiteInfo> {
    const { site } = await this.query<{ site: { id: string; name: string; timezone: string } }>('sites.get', [['site', id]])
    return { id, name: site.name, timezone: site.timezone }
  }

  /**
   * One call per site. The main cut gives pageviews and visitor-days per local day; two panels
   * give the top paths and countries by pageviews over the whole range.
   */
  async explore(site: SiteInfo, from: string, to: string) {
    const page = { size: TOP_SIZE, index: 0 }
    const top = (field: string) =>
      JSON.stringify({
        measures: ['pageviews'],
        dimensions: [field],
        sort: [{ by: 'pageviews', dir: 'desc' }],
        page,
      })
    const state = JSON.stringify({
      v: 1,
      site: site.id,
      measures: ['pageviews', 'visitor_days'],
      dimensions: [],
      filter: null,
      range: { kind: 'days', start: from, end: to },
      timezone: site.timezone,
      granularity: 'day',
      compare: 'none',
      metric_version: 1,
      sort: [],
      page: { size: 50, index: 0 },
      visualization: 'line',
    })
    const result = await this.query<ExploreResult>('analytics.explore', [
      ['state', state],
      ['panels', top('path')],
      ['panels', top('country')],
    ])
    const days = new Map<string, DayCounts>()
    for (const point of result.main.series?.points ?? []) {
      days.set(point.local.slice(0, 10), { pageviews: count(point.values[0]), visitors: count(point.values[1]) })
    }
    const keys = (cut: Cut | undefined, upper: boolean) =>
      (cut?.rows ?? [])
        .map((r) => r.key[0])
        .filter((c): c is Cell & { v: string } => c?.t === 'string' && !!c.v)
        .map((c) => (upper ? c.v.toUpperCase() : c.v))
    return { days, pages: keys(result.panels[0], false), countries: keys(result.panels[1], true) }
  }
}
