#!/usr/bin/env bun
import { HolocronApi, HolocronError, type SiteInfo } from './holocron.ts'
import {
  addDays,
  classifyDays,
  compareTop,
  daysBetween,
  isDay,
  isTimezone,
  localDay,
  MAX_DAYS,
  percentDiff,
  siteProblems,
  TOLERANCE,
  verdictLine,
  type SiteReport,
  type TopComparison,
} from './lib.ts'
import { UmamiSource } from './umami.ts'

const USAGE = `Usage: bun compare.ts --umami <mysql|postgres url> --holocron <url> --from YYYY-MM-DD [--to YYYY-MM-DD] [--site <uuid>]... [--json]

Environment:
  HOLOCRON_API_KEY  API key with analytics.explore, sites.get and sites.list (required)
  UMAMI_PASSWORD    database password, when the Umami URL has none

Exit codes: 0 within tolerance, 1 not, 2 usage or connection error.`

class UsageError extends Error {}

interface Options {
  umami: URL
  holocron: URL
  apiKey: string
  from: string
  to: string | null
  sites: string[]
  json: boolean
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/

function parseUrl(label: string, value: string | undefined, protocols: string[]): URL {
  if (!value) throw new UsageError(`${label} is required`)
  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw new UsageError(`${label} is not a URL`)
  }
  if (!protocols.includes(url.protocol)) throw new UsageError(`${label} must start with ${protocols.join(' or ')}`)
  return url
}

export function parseOptions(argv: string[], env: Record<string, string | undefined>): Options {
  const flags: Record<string, string[]> = {}
  let json = false
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]!
    if (arg === '--json') json = true
    else if (['--umami', '--holocron', '--from', '--to', '--site'].includes(arg)) {
      const value = argv[++i]
      if (value === undefined) throw new UsageError(`${arg} needs a value`)
      ;(flags[arg] ??= []).push(value)
    } else throw new UsageError(`unknown argument ${arg}`)
  }
  for (const single of ['--umami', '--holocron', '--from', '--to']) {
    if ((flags[single]?.length ?? 0) > 1) throw new UsageError(`${single} given twice`)
  }
  const umami = parseUrl('--umami', flags['--umami']?.[0], ['mysql:', 'postgres:', 'postgresql:'])
  if (!umami.password && env.UMAMI_PASSWORD) umami.password = env.UMAMI_PASSWORD
  const holocron = parseUrl('--holocron', flags['--holocron']?.[0], ['http:', 'https:'])
  const apiKey = env.HOLOCRON_API_KEY?.trim()
  if (!apiKey) throw new UsageError('HOLOCRON_API_KEY is not set')
  const from = flags['--from']?.[0]
  if (!from || !isDay(from)) throw new UsageError('--from must be a date as YYYY-MM-DD')
  const to = flags['--to']?.[0] ?? null
  if (to !== null && !isDay(to)) throw new UsageError('--to must be a date as YYYY-MM-DD')
  if (to !== null && to < from) throw new UsageError('--to is before --from')
  if (to !== null && daysBetween(from, to).length > MAX_DAYS) throw new UsageError(`the range is over ${MAX_DAYS} days`)
  const sites = (flags['--site'] ?? []).map((s) => s.toLowerCase())
  for (const site of sites) if (!UUID.test(site)) throw new UsageError(`--site ${site} is not a UUID`)
  return { umami, holocron, apiKey, from, to, sites: [...new Set(sites)], json }
}

async function compareSite(umami: UmamiSource, holocron: HolocronApi, site: SiteInfo, from: string, to: string | null): Promise<SiteReport> {
  const last = to ?? addDays(localDay(Date.now(), site.timezone), -1)
  if (last < from) throw new UsageError(`${site.name || site.id}: no finished day from ${from} yet`)
  if (daysBetween(from, last).length > MAX_DAYS) throw new UsageError(`the range is over ${MAX_DAYS} days`)
  const days = daysBetween(from, last)
  const h = await holocron.explore(site, from, last)
  const umamiDays = await umami.days(site.id, days, site.timezone)
  const { rows, outages } = classifyDays(days, umamiDays, h.days)
  const pages = compareTop(await umami.topPages(site.id, from, last, site.timezone), h.pages)
  const countries = compareTop(await umami.topCountries(site.id, from, last, site.timezone), h.countries)
  const report = { site: site.id, name: site.name, timezone: site.timezone, days: rows, outages, pages, countries }
  return { ...report, problems: siteProblems(report) }
}

const pct = (u: number, h: number) => {
  const p = percentDiff(u, h)
  return p === null ? 'n/a' : `${p >= 0 ? '+' : ''}${p.toFixed(1)}%`
}

function table(rows: string[][], right: number[]): string {
  const widths = rows[0]!.map((_, c) => Math.max(...rows.map((r) => r[c]!.length)))
  return rows
    .map((r) => '  ' + r.map((cell, c) => (right.includes(c) ? cell.padStart(widths[c]!) : cell.padEnd(widths[c]!))).join('  ').trimEnd())
    .join('\n')
}

function topLines(label: string, top: TopComparison): string[] {
  const lines = [`  ${label}: ${top.verdict === 'ok' ? 'ok' : 'differs'}, overlap ${Math.round(top.overlap * 100)}%, rank agreement ${Math.round(top.rankAgreement * 100)}%`]
  if (top.onlyUmami.length) lines.push(`    only in Umami's top 10: ${top.onlyUmami.join(', ')}`)
  if (top.onlyHolocron.length) lines.push(`    only in Holocron's top 10: ${top.onlyHolocron.join(', ')}`)
  return lines
}

function render(reports: SiteReport[], notes: string[], from: string): string {
  const out: string[] = []
  for (const r of reports) {
    out.push(`${r.name || r.site}  ${r.site}  (${r.timezone}, from ${from})`)
    if (r.days.length) {
      const rows = [['day', 'pv umami', 'pv holocron', 'diff', 'visitors umami', 'visitor-days holocron', 'diff', 'status']]
      for (const d of r.days) {
        const bad = [d.pageviews === 'differs' ? 'pageviews' : '', d.visitors === 'differs' ? 'visitors' : ''].filter(Boolean)
        rows.push([
          d.day,
          String(d.umami.pageviews),
          String(d.holocron.pageviews),
          pct(d.umami.pageviews, d.holocron.pageviews),
          String(d.umami.visitors),
          String(d.holocron.visitors),
          pct(d.umami.visitors, d.holocron.visitors),
          bad.length ? `OFF ${bad.join(', ')}` : 'ok',
        ])
      }
      out.push(table(rows, [1, 2, 3, 4, 5, 6]))
    } else out.push('  no day with traffic on both sides')
    out.push(...topLines('top pages', r.pages), ...topLines('top countries', r.countries))
    if (r.outages.length) {
      out.push('  possible outages (a side has no pageviews, not counted as differences):')
      for (const o of r.outages) {
        const who = o.zero === 'both' ? 'no pageviews on either side' : o.zero === 'umami' ? 'Umami has none' : 'Holocron has none'
        out.push(`    ${o.day}  umami ${o.umami.pageviews}, holocron ${o.holocron.pageviews}  (${who})`)
      }
    }
    out.push(`  ${r.problems.length ? `OFF: ${r.problems.join('; ')}` : 'within tolerance'}`, '')
  }
  out.push(
    'Notes:',
    '  Holocron visitors are visitor-days (daily pseudonyms). Umami visitors are distinct sessions per day.',
    `  The mirror drops Umami's cache token, so session continuity differs. Visitors are allowed ${TOLERANCE.visitors * 100}%, pageviews ${TOLERANCE.pageviews * 100}%.`,
    `  Top lists are the whole range, by pageviews. Pass: ${TOLERANCE.topOverlap * 100}% shared entries and ${TOLERANCE.topRank * 100}% of those within ${TOLERANCE.rankSlack} ranks.`,
    ...notes.map((n) => `  ${n}`),
    '',
    verdictLine(reports),
  )
  return out.join('\n')
}

async function main(): Promise<number> {
  if (process.argv.includes('--help') || process.argv.includes('-h')) {
    console.log(USAGE)
    return 0
  }
  let options: Options
  try {
    options = parseOptions(process.argv.slice(2), process.env)
  } catch (error) {
    if (!(error instanceof UsageError)) throw error
    console.error(`error: ${error.message}\n\n${USAGE}`)
    return 2
  }
  const api = new HolocronApi(options.holocron, options.apiKey)
  let umami: UmamiSource | null = null
  try {
    try {
      umami = await UmamiSource.open(options.umami)
    } catch (error) {
      const host = `${options.umami.hostname}${options.umami.port ? `:${options.umami.port}` : ''}`
      throw new HolocronError(`cannot read Umami at ${host}: ${(error as Error).message}`, 'connection')
    }
    const [umamiSites, holocronSites] = await Promise.all([umami.websites(), api.sites()])
    const both = umamiSites.filter((id) => holocronSites.includes(id))
    const notes: string[] = []
    for (const id of options.sites) {
      if (!both.includes(id)) throw new UsageError(`site ${id} is not on both sides`)
    }
    const ids = options.sites.length ? options.sites : both
    if (!options.sites.length) {
      const onlyUmami = umamiSites.filter((id) => !holocronSites.includes(id))
      const onlyHolocron = holocronSites.filter((id) => !umamiSites.includes(id))
      if (onlyUmami.length) notes.push(`Sites only in Umami, skipped: ${onlyUmami.join(', ')}`)
      if (onlyHolocron.length) notes.push(`Sites only in Holocron, skipped: ${onlyHolocron.join(', ')}`)
    }
    const reports: SiteReport[] = []
    for (const id of ids) {
      const site = await api.site(id)
      if (!isTimezone(site.timezone)) throw new HolocronError(`site ${id} has an unknown timezone ${site.timezone}`, 'refused')
      reports.push(await compareSite(umami, api, site, options.from, options.to))
    }
    const ok = reports.every((r) => r.problems.length === 0)
    if (options.json) {
      console.log(JSON.stringify({ from: options.from, to: options.to, tolerance: TOLERANCE, within_tolerance: ok, notes, sites: reports }, null, 2))
    } else console.log(render(reports, notes, options.from))
    return ok ? 0 : 1
  } catch (error) {
    if (error instanceof UsageError || error instanceof HolocronError) {
      console.error(`error: ${error.message}`)
      return 2
    }
    throw error
  } finally {
    await umami?.close().catch(() => {})
  }
}

if (import.meta.main) process.exit(await main())
