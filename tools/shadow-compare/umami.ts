import { SQL } from 'bun'
import { dayBounds, TOP_SIZE, type DayCounts } from './lib.ts'

/** Read-only view of an Umami database (MySQL or PostgreSQL, schema 2.x or 3.x). */
export class UmamiSource {
  private constructor(
    private readonly db: SQL,
    private readonly pg: boolean,
  ) {}

  static async open(url: URL): Promise<UmamiSource> {
    const pg = url.protocol === 'postgres:' || url.protocol === 'postgresql:'
    const source = new UmamiSource(new SQL(url.toString()), pg)
    await source.query('SELECT 1 AS ok', [])
    return source
  }

  async close(): Promise<void> {
    await this.db.close()
  }

  private async query(text: string, params: (string | number)[]): Promise<Record<string, unknown>[]> {
    return (await this.db.unsafe(text, params)) as Record<string, unknown>[]
  }

  /** `?` for MySQL, `$n` for PostgreSQL. */
  private ph(n: number, cast = ''): string {
    return this.pg ? `$${n}${cast}` : '?'
  }

  /** UTC instants as the column type expects: a plain DATETIME string on MySQL. */
  private instant(ms: number): string {
    const iso = new Date(ms).toISOString()
    return this.pg ? iso : iso.slice(0, 19).replace('T', ' ')
  }

  /** IDs of websites that are not deleted, lower case. */
  async websites(): Promise<string[]> {
    const id = this.pg ? 'website_id::text' : 'CAST(website_id AS CHAR)'
    const rows = await this.query(`SELECT ${id} AS id FROM website WHERE deleted_at IS NULL`, [])
    return rows.map((r) => String(r.id).toLowerCase())
  }

  private window(site: string, start: number, end: number, types: string) {
    return {
      where: `e.website_id = ${this.ph(1, '::uuid')} AND e.event_type IN (${types}) AND e.created_at >= ${this.ph(2, '::timestamptz')} AND e.created_at < ${this.ph(3, '::timestamptz')}`,
      params: [site, this.instant(start), this.instant(end)],
    }
  }

  /** Pageviews and distinct sessions (pageviews and custom events) for each local day. */
  async days(site: string, days: string[], timezone: string): Promise<Map<string, DayCounts>> {
    const result = new Map<string, DayCounts>()
    for (const day of days) {
      const { start, end } = dayBounds(day, timezone)
      const { where, params } = this.window(site, start, end, '1, 2')
      const [row] = await this.query(
        `SELECT COALESCE(SUM(CASE WHEN e.event_type = 1 THEN 1 ELSE 0 END), 0) AS pageviews,
                COUNT(DISTINCT e.session_id) AS visitors
         FROM website_event e WHERE ${where}`,
        params,
      )
      result.set(day, { pageviews: Number(row?.pageviews ?? 0), visitors: Number(row?.visitors ?? 0) })
    }
    return result
  }

  /** Most viewed paths over the whole range, ties by path. */
  async topPages(site: string, from: string, to: string, timezone: string): Promise<string[]> {
    const { where, params } = this.range(site, from, to, timezone)
    const rows = await this.query(
      `SELECT e.url_path AS k, COUNT(*) AS c FROM website_event e WHERE ${where} AND e.url_path IS NOT NULL
       GROUP BY e.url_path ORDER BY c DESC, k ASC LIMIT ${TOP_SIZE}`,
      params,
    )
    return rows.map((r) => String(r.k))
  }

  /** Countries by pageviews over the whole range, from the session, ties by code. */
  async topCountries(site: string, from: string, to: string, timezone: string): Promise<string[]> {
    const { where, params } = this.range(site, from, to, timezone)
    const rows = await this.query(
      `SELECT s.country AS k, COUNT(*) AS c FROM website_event e JOIN session s ON s.session_id = e.session_id
       WHERE ${where} AND s.country IS NOT NULL AND s.country <> ''
       GROUP BY s.country ORDER BY c DESC, k ASC LIMIT ${TOP_SIZE}`,
      params,
    )
    return rows.map((r) => String(r.k).toUpperCase())
  }

  private range(site: string, from: string, to: string, timezone: string) {
    return this.window(site, dayBounds(from, timezone).start, dayBounds(to, timezone).end, '1')
  }
}
