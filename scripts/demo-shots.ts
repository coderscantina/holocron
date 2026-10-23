/**
 * Captures the README screenshots, the walkthrough GIF and the GitHub social card from the demo
 * instance `scripts/demo-seed.sh` builds, seeds and serves on a frozen clock. Start the seed, wait
 * about 60 s for the first summary round, then:
 *
 *   bun scripts/demo-shots.ts [--dir /tmp/holocron-demo]
 *
 * Before the shots it sends live visitors to lagoon.test through the public collection API
 * (`/api/send`, `/api/record`): visitors from several cities for the realtime globe, and one
 * desktop visit with a styled replay, since the seeded replays carry an empty page. The server
 * stamps them with its frozen instant, and the browser clock is pinned to the same instant, so
 * "today" and "just now" line up. Every run adds these visitors again; reseed for a clean slate.
 *
 * Writes `docs/assets/*.png` (1440x900 at 2x, dark mode), `docs/assets/walkthrough.gif` and
 * `.github/social-preview.png` from `scripts/social-preview.html`.
 * Uses the web app's Playwright and `ffmpeg` for the GIF and PNG compression.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { parseArgs } from 'node:util'

import { chromium, type Browser, type BrowserContext, type BrowserContextOptions, type Page } from '../apps/web/node_modules/playwright'

import type { RecordingList, SiteSummary, WorkspaceList, WorkspaceSummary } from '../apps/web/src/generated'

const repo = resolve(import.meta.dirname, '..')
const assets = join(repo, 'docs', 'assets')
const socialCard = join(repo, '.github', 'social-preview.png')
const { values: options } = parseArgs({ options: { dir: { type: 'string', default: '/tmp/holocron-demo' } } })
const demo = JSON.parse(readFileSync(join(resolve(options.dir), 'demo.json'), 'utf8')) as {
  url: string
  email: string
  password: string
  now: string
}
const now = Date.parse(demo.now)

const desktop: BrowserContextOptions = {
  baseURL: demo.url,
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
  timezoneId: 'Europe/Vienna',
  locale: 'en-GB',
  colorScheme: 'dark',
}

/** A context on the server's frozen instant. The clock keeps running from there, so replays play. */
async function pinnedContext(browser: Browser, extra: BrowserContextOptions = {}): Promise<BrowserContext> {
  const context = await browser.newContext({ ...desktop, ...extra })
  await context.clock.install({ time: now })
  return context
}

/** Waits for fonts, two frames and every finite time-based animation, so a shot sees the settled page. */
async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready)
  await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => animation.timeline instanceof DocumentTimeline)
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined))
    )
  )
  await page.waitForTimeout(300)
}

/** Waits until the page has drawn and no loading skeleton is left. */
async function loaded(page: Page) {
  await page.waitForLoadState('load')
  await page.waitForFunction(() => document.querySelector('main') && !document.querySelector('[data-slot="skeleton"]'), undefined, { timeout: 20_000 })
}

async function shot(page: Page, name: string) {
  await loaded(page)
  // The app scrolls inside its own panes; a click may have moved one.
  await page.evaluate(() => document.querySelectorAll('*').forEach((element) => element.scrollTo(0, 0)))
  await settle(page)
  await page.screenshot({ path: join(assets, `${name}.png`) })
  console.log(`wrote ${name}.png`)
}

async function signIn(browser: Browser): Promise<string> {
  const context = await pinnedContext(browser)
  const page = await context.newPage()
  await page.goto('/login')
  await page.getByLabel('Email').fill(demo.email)
  await page.getByLabel('Password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))
  const state = join(mkdtempSync(join(tmpdir(), 'holocron-shots-')), 'state.json')
  await context.storageState({ path: state })
  await context.close()
  return state
}

interface Demo {
  workspace: WorkspaceSummary
  site: SiteSummary
}

/** The Lagoon team and its main site, read through the API with the signed-in session. */
async function lagoon(page: Page): Promise<Demo> {
  const response = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await response.json()) as WorkspaceList
  const workspace = workspaces.find((entry) => entry.name === 'Lagoon')
  const site = workspace?.sites.find((entry) => entry.domain === 'lagoon.test')
  if (!workspace || !site) throw new Error('no Lagoon workspace with lagoon.test; run scripts/demo-seed.sh first')
  return { workspace, site }
}

// Live traffic ---------------------------------------------------------------------------------

interface Place {
  country: string
  city: string
  latitude: number
  longitude: number
  language: string
}

const places: Place[] = [
  { country: 'AT', city: 'Vienna', latitude: 48.21, longitude: 16.37, language: 'de-AT' },
  { country: 'AT', city: 'Salzburg', latitude: 47.81, longitude: 13.04, language: 'de-AT' },
  { country: 'AT', city: 'Graz', latitude: 47.07, longitude: 15.44, language: 'de-AT' },
  { country: 'AT', city: 'Linz', latitude: 48.31, longitude: 14.29, language: 'de-AT' },
  { country: 'DE', city: 'Munich', latitude: 48.14, longitude: 11.58, language: 'de-DE' },
  { country: 'DE', city: 'Berlin', latitude: 52.52, longitude: 13.4, language: 'de-DE' },
  { country: 'CH', city: 'Zurich', latitude: 47.38, longitude: 8.54, language: 'de-CH' },
  { country: 'IT', city: 'Milan', latitude: 45.46, longitude: 9.19, language: 'it-IT' },
  { country: 'CZ', city: 'Prague', latitude: 50.08, longitude: 14.44, language: 'cs-CZ' },
  { country: 'HU', city: 'Budapest', latitude: 47.5, longitude: 19.04, language: 'hu-HU' },
  { country: 'NL', city: 'Amsterdam', latitude: 52.37, longitude: 4.9, language: 'nl-NL' },
  { country: 'GB', city: 'London', latitude: 51.51, longitude: -0.13, language: 'en-GB' },
  { country: 'SE', city: 'Stockholm', latitude: 59.33, longitude: 18.07, language: 'sv-SE' },
  { country: 'US', city: 'New York', latitude: 40.71, longitude: -74.01, language: 'en-US' },
  { country: 'CA', city: 'Toronto', latitude: 43.65, longitude: -79.38, language: 'en-CA' },
  { country: 'JP', city: 'Tokyo', latitude: 35.68, longitude: 139.69, language: 'ja-JP' },
]

const agents = [
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36',
  'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36',
  'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36',
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0',
]

const titles: Record<string, string> = {
  '/de': 'Lagoon am Attersee',
  '/de/events': 'Programm | Lagoon',
  '/de/events/seefest': 'Seefest 2026 | Lagoon',
  '/de/anreise': 'Anreise | Lagoon',
  '/de/newsletter/herbst': 'Herbst am See | Lagoon',
  '/de/checkout': 'Tickets | Lagoon',
  '/en': 'Lagoon on Lake Attersee',
}

/** Each visitor's pages, in order; a trailing `signup` is the custom event. */
const visits: string[][] = [
  ['/de', '/de/events'],
  ['/de/events/seefest', '/de/checkout'],
  ['/de/newsletter/herbst', 'signup'],
  ['/en'],
  ['/de/anreise'],
  ['/de', '/de/events/seefest'],
  ['/de/events'],
  ['/de/newsletter/herbst', '/de/events/seefest', 'signup'],
  ['/de/checkout'],
  ['/en', '/de/events'],
  ['/de'],
  ['/de/events/seefest'],
]

interface Visitor {
  ip: string
  agent: string
  place: Place
  token?: string
}

interface Sent {
  cache: string
  visitId: string
}

/** One `/api/send` hit from `visitor`, through 127.0.0.1 as its trusted proxy with CDN geo headers. */
async function send(site: SiteSummary, visitor: Visitor, hit: { path: string; event?: string; referrer?: string; at?: number }): Promise<Sent> {
  const headers: Record<string, string> = {
    'content-type': 'application/json',
    'user-agent': visitor.agent,
    'x-forwarded-for': visitor.ip,
    'cf-ipcountry': visitor.place.country,
    'cf-ipcity': visitor.place.city,
    'cf-iplatitude': String(visitor.place.latitude),
    'cf-iplongitude': String(visitor.place.longitude),
  }
  if (visitor.token) headers['x-umami-cache'] = visitor.token
  const payload = {
    website: site.id,
    hostname: site.domain,
    language: visitor.place.language,
    screen: visitor.agent.includes('Mobile') ? '390x844' : '1440x900',
    title: titles[hit.path] ?? 'Lagoon',
    url: hit.path,
    referrer: hit.referrer ?? '',
    ...(hit.event ? { name: hit.event } : {}),
    // Seconds, as the Umami tracker sends it; without it the server stamps its frozen instant.
    ...(hit.at ? { timestamp: Math.floor(hit.at / 1000) } : {}),
  }
  const response = await fetch(`${demo.url}/api/send`, { method: 'POST', headers, body: JSON.stringify({ type: 'event', payload }) })
  if (!response.ok) throw new Error(`/api/send answered ${response.status}: ${await response.text()}`)
  const sent = (await response.json()) as Sent
  visitor.token = sent.cache
  return sent
}

/**
 * Visitors from around the world on lagoon.test: half within the last four minutes, so they count
 * as current, the other half spread over the half hour the realtime chart shows.
 */
async function liveVisitors(site: SiteSummary) {
  for (const [index, pages] of visits.entries()) {
    for (const round of [0, 1]) {
      const n = index * 2 + round
      const visitor: Visitor = { ip: `198.51.100.${10 + n}`, agent: agents[n % agents.length]!, place: places[n % places.length]! }
      const start = now - (round === 0 ? 240_000 - index * 18_000 : 1_740_000 - index * 130_000)
      let path = '/de'
      for (const [step, name] of pages.entries()) {
        const at = start + step * 25_000
        if (name === 'signup') await send(site, visitor, { path, event: 'signup', at })
        else await send(site, visitor, { path: (path = name), at })
      }
    }
  }
}

// A styled replay ------------------------------------------------------------------------------

/** rrweb's serialized DOM, the subset a static page needs. */
type SerializedNode =
  | { type: 0; id: number; childNodes: SerializedNode[] }
  | { type: 1; id: number; name: string; publicId: string; systemId: string }
  | { type: 2; id: number; tagName: string; attributes: Record<string, string>; childNodes: SerializedNode[] }
  | { type: 3; id: number; textContent: string; isStyle?: true }

type ReplayEvent = { type: number; timestamp: number; data: Record<string, unknown> }

const pageStyle = `
*{box-sizing:border-box;margin:0}
body{font-family:-apple-system,"Segoe UI",Helvetica,Arial,sans-serif;color:#10242b;background:#f3f6f5}
header{display:flex;align-items:center;justify-content:space-between;padding:22px 64px;background:#0d3b45;color:#fff}
.logo{font-weight:800;font-size:22px;letter-spacing:-.02em}
nav a{color:#cfe6e3;margin-left:28px;font-size:15px;text-decoration:none}
.hero{padding:84px 64px 92px;background:linear-gradient(160deg,#0d3b45 0%,#1b6f73 55%,#e3a65b 130%);color:#fff}
.hero.small{padding:56px 64px 60px}
.eyebrow{font-size:14px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:#f2b45a}
.hero h1{font-size:60px;line-height:1.05;letter-spacing:-.03em;max-width:760px;margin-top:12px}
.hero p{font-size:20px;line-height:1.5;max-width:580px;margin-top:20px;color:#d9eeeb}
.btn{display:inline-block;margin-top:30px;padding:14px 26px;border-radius:8px;background:#f2b45a;color:#10242b;font-weight:700;font-size:16px;text-decoration:none}
.grid{display:grid;grid-template-columns:repeat(3,1fr);gap:24px;padding:56px 64px}
.card{background:#fff;border-radius:12px;overflow:hidden;box-shadow:0 1px 3px rgba(16,36,43,.08)}
.card .img{height:150px}
.card h3{font-size:19px;padding:18px 20px 4px}
.card p{padding:0 20px 20px;color:#4b6168;font-size:15px;line-height:1.5}
.cols{display:grid;grid-template-columns:1.6fr 1fr;gap:40px;padding:48px 64px}
.cols h2{font-size:26px;margin-bottom:14px}
.cols p{font-size:17px;line-height:1.65;color:#33494f;margin-bottom:16px}
.box{background:#fff;border-radius:12px;padding:28px;box-shadow:0 1px 3px rgba(16,36,43,.08)}
.row{display:flex;justify-content:space-between;padding:14px 0;border-bottom:1px solid #e3eae8;font-size:16px}
.row b{font-weight:700}
label{display:block;font-size:14px;font-weight:600;margin:18px 0 6px;color:#33494f}
input{width:100%;padding:12px 14px;border:1px solid #c9d6d3;border-radius:8px;font-size:16px;background:#fff}
.btn.wide{display:block;text-align:center}
`

/** Builds rrweb nodes with ascending IDs. Strings become text nodes. */
class Dom {
  private next = 1
  private id() {
    return this.next++
  }
  el(tagName: string, attributes: Record<string, string>, ...children: (SerializedNode | string)[]): Extract<SerializedNode, { type: 2 }> {
    const id = this.id()
    const childNodes = children.map((child): SerializedNode => (typeof child === 'string' ? { type: 3, id: this.id(), textContent: child } : child))
    return { type: 2, id, tagName, attributes, childNodes }
  }
  document(body: SerializedNode) {
    const id = this.id()
    const style: SerializedNode = { type: 3, id: this.id(), textContent: pageStyle, isStyle: true }
    const head = this.el('head', {}, this.el('meta', { charset: 'utf-8' }), { ...this.el('style', {}), childNodes: [style] })
    const html = this.el('html', { lang: 'de' }, head, body)
    const node: SerializedNode = { type: 0, id, childNodes: [{ type: 1, id: this.id(), name: 'html', publicId: '', systemId: '' }, html] }
    return { node, id }
  }
}

const header = (dom: Dom) =>
  dom.el('header', {}, dom.el('div', { class: 'logo' }, 'Lagoon'), dom.el('nav', {}, ...['Programm', 'Seefest', 'Anreise', 'Tickets'].map((name) => dom.el('a', { href: '#' }, name))))

const card = (dom: Dom, title: string, text: string, colors: string) =>
  dom.el('div', { class: 'card' }, dom.el('div', { class: 'img', style: `background:linear-gradient(135deg,${colors})` }), dom.el('h3', {}, title), dom.el('p', {}, text))

function newsletterPage() {
  const dom = new Dom()
  const cta = dom.el('a', { class: 'btn', href: '#' }, 'Zum Seefest')
  const body = dom.el(
    'body',
    {},
    header(dom),
    dom.el(
      'section',
      { class: 'hero' },
      dom.el('div', { class: 'eyebrow' }, 'Newsletter Herbst'),
      dom.el('h1', {}, 'Herbst am See: drei Abende, ein Ufer'),
      dom.el('p', {}, 'Laternen auf dem Wasser, Jazz am Steg und das Seefest zum Saisonende. Für Abonnentinnen und Abonnenten mit 15 % Frühbucherrabatt.'),
      cta
    ),
    dom.el(
      'div',
      { class: 'grid' },
      card(dom, 'Seefest 2026', 'Sa, 24. Oktober · Live-Musik, Feuerwerk und Fischerboote', '#1b6f73,#e3a65b'),
      card(dom, 'Jazz am Steg', 'Fr, 30. Oktober · Das Trio Wellenschlag spielt bis Mitternacht', '#0d3b45,#5f8fa0'),
      card(dom, 'Laternenfahrt', 'So, 8. November · Mit dem Raddampfer in die Dämmerung', '#3c2f5c,#f2b45a')
    )
  )
  return { ...dom.document(body), cta, body }
}

function eventPage() {
  const dom = new Dom()
  const buy = dom.el('a', { class: 'btn wide', href: '#' }, 'Tickets kaufen')
  const body = dom.el(
    'body',
    {},
    header(dom),
    dom.el('section', { class: 'hero small' }, dom.el('div', { class: 'eyebrow' }, 'Sa, 24. Oktober 2026 · ab 16 Uhr'), dom.el('h1', {}, 'Seefest 2026')),
    dom.el(
      'div',
      { class: 'cols' },
      dom.el(
        'div',
        {},
        dom.el('h2', {}, 'Das Fest zum Saisonende'),
        dom.el('p', {}, 'Zum letzten Mal in diesem Jahr legen die Fischerboote am Lagoon-Ufer an. Drei Bühnen, regionale Küche und um 21 Uhr das Feuerwerk über dem See.'),
        dom.el('p', {}, 'Die Schifffahrt bringt Sie ab Unterach und Nußdorf direkt zum Fest. Parkplätze gibt es beim Bahnhof, von dort fährt alle 15 Minuten ein Shuttle.')
      ),
      dom.el(
        'div',
        { class: 'box' },
        dom.el('div', { class: 'row' }, 'Tagesticket', dom.el('b', {}, '€ 39')),
        dom.el('div', { class: 'row' }, 'Ticket mit Schifffahrt', dom.el('b', {}, '€ 52')),
        dom.el('div', { class: 'row' }, 'Kinder bis 12', dom.el('b', {}, 'frei')),
        buy
      )
    )
  )
  return { ...dom.document(body), buy, body }
}

function checkoutPage() {
  const dom = new Dom()
  const name = dom.el('input', { type: 'text', value: '' })
  const email = dom.el('input', { type: 'email', value: '' })
  const pay = dom.el('a', { class: 'btn wide', href: '#' }, 'Jetzt bezahlen')
  const body = dom.el(
    'body',
    {},
    header(dom),
    dom.el(
      'div',
      { class: 'cols' },
      dom.el('div', { class: 'box' }, dom.el('h2', {}, 'Ihre Daten'), dom.el('label', {}, 'Name'), name, dom.el('label', {}, 'E-Mail'), email),
      dom.el(
        'div',
        { class: 'box' },
        dom.el('h2', {}, 'Bestellung'),
        dom.el('div', { class: 'row' }, '2 × Ticket mit Schifffahrt', dom.el('b', {}, '€ 104')),
        dom.el('div', { class: 'row' }, 'Newsletter-Rabatt 15 %', dom.el('b', {}, '− € 15,60')),
        dom.el('div', { class: 'row' }, 'Gesamt', dom.el('b', {}, '€ 88,40')),
        pay
      )
    )
  )
  return { ...dom.document(body), name, email, pay, body }
}

/** A 40 s desktop visit: the newsletter page, the Seefest and the checkout, with moves, scrolls, clicks and typing. */
function replayEvents(start: number): ReplayEvent[] {
  const events: ReplayEvent[] = []
  const at = (ms: number, type: number, data: Record<string, unknown>) => events.push({ type, timestamp: start + ms, data })
  const page = (ms: number, path: string, snapshot: { node: SerializedNode }) => {
    at(ms, 4, { href: `https://lagoon.test${path}`, width: 1280, height: 800 })
    at(ms + 20, 2, { node: snapshot.node, initialOffset: { left: 0, top: 0 } })
  }
  /** Mouse moves along a path of points, one batch per call. */
  const move = (ms: number, target: number, points: [number, number][]) =>
    at(ms + points.length * 120, 3, { source: 1, positions: points.map(([x, y], i) => ({ x, y, id: target, timeOffset: (i - points.length) * 120 })) })
  const click = (ms: number, target: number, x: number, y: number) => {
    at(ms, 3, { source: 2, type: 1, id: target, x, y })
    at(ms + 80, 3, { source: 2, type: 0, id: target, x, y })
    at(ms + 90, 3, { source: 2, type: 2, id: target, x, y })
  }
  const scroll = (ms: number, target: number, y: number) => at(ms, 3, { source: 3, id: target, x: 0, y })
  const type = (ms: number, target: number, text: string) => {
    for (let i = 1; i <= text.length; i++) at(ms + i * 140, 3, { source: 5, id: target, text: '*'.repeat(i), isChecked: false })
  }

  const news = newsletterPage()
  page(0, '/de/newsletter/herbst', news)
  move(400, news.body.id, [[900, 160], [820, 220], [700, 300], [560, 380], [420, 430], [300, 470], [240, 500]])
  move(2600, news.body.id, [[260, 520], [320, 560], [420, 600], [520, 640], [640, 660]])
  scroll(4600, news.id, 240)
  scroll(5200, news.id, 520)
  move(5800, news.body.id, [[640, 520], [600, 440], [520, 360], [420, 300]])
  scroll(7600, news.id, 120)
  scroll(8000, news.id, 0)
  move(8400, news.body.id, [[380, 380], [300, 420], [220, 450], [160, 470], [140, 476]])
  click(9600, news.cta.id, 140, 476)

  const seefest = eventPage()
  page(10_200, '/de/events/seefest', seefest)
  move(10_800, seefest.body.id, [[300, 300], [380, 360], [460, 420], [520, 480], [560, 520]])
  scroll(13_000, seefest.id, 180)
  move(14_000, seefest.body.id, [[600, 420], [720, 400], [840, 380], [960, 360], [1040, 360]])
  move(16_400, seefest.body.id, [[1040, 380], [1020, 420], [1000, 460], [980, 500], [970, 520]])
  click(17_600, seefest.buy.id, 970, 520)

  const checkout = checkoutPage()
  page(18_200, '/de/checkout', checkout)
  move(18_800, checkout.body.id, [[700, 300], [560, 260], [420, 230], [320, 210]])
  click(19_800, checkout.name.id, 320, 210)
  type(20_000, checkout.name.id, 'Anna Gruber')
  move(22_000, checkout.body.id, [[320, 240], [320, 270], [320, 290]])
  click(22_800, checkout.email.id, 320, 292)
  type(23_000, checkout.email.id, 'anna.gruber@posteo.at')
  move(26_400, checkout.body.id, [[400, 320], [600, 360], [800, 380], [940, 390], [980, 392]])
  move(29_000, checkout.body.id, [[980, 400], [980, 420], [975, 440]])
  click(30_200, checkout.pay.id, 975, 440)
  move(31_000, checkout.body.id, [[960, 420], [920, 380], [880, 340]])
  return events
}

/** One desktop visit from the newsletter, with its replay; returns the replay's ID. */
async function recordedVisit(site: SiteSummary): Promise<string> {
  const visitor: Visitor = { ip: '198.51.100.200', agent: agents[0]!, place: places[0]! }
  const { visitId } = await send(site, visitor, { path: '/de/newsletter/herbst', referrer: 'https://news.example.net/lagoon/herbst' })
  await send(site, visitor, { path: '/de/events/seefest' })
  await send(site, visitor, { path: '/de/checkout' })
  await send(site, visitor, { path: '/de/checkout', event: 'signup' })
  const events = replayEvents(now - 45_000)
  const response = await fetch(`${demo.url}/api/record`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'user-agent': visitor.agent, 'x-forwarded-for': visitor.ip, 'cf-ipcountry': 'AT', 'x-umami-cache': visitor.token ?? '' },
    body: JSON.stringify({ type: 'record', payload: { website: site.id, events, timestamp: 0 } }),
  })
  const answer = (await response.json()) as { ok?: boolean; reason?: string }
  if (!response.ok || answer.ok === false) throw new Error(`/api/record answered ${response.status}: ${JSON.stringify(answer)}`)
  return visitId
}

/** Waits until the replay shows up in the site's list, once collection has committed it. */
async function waitForReplay(page: Page, site: SiteSummary, capture: string) {
  for (let attempt = 0; attempt < 30; attempt++) {
    const response = await page.request.get(`/api/queries/recordings.list?site=${site.id}&start=${now - 86_400_000}&end=${now + 60_000}`, {
      headers: { 'x-holocron-request': '1' },
    })
    const { captures } = (await response.json()) as RecordingList
    if (captures.some((entry) => entry.id === capture)) return
    await page.waitForTimeout(1000)
  }
  throw new Error(`replay ${capture} never appeared`)
}

// Shots ----------------------------------------------------------------------------------------

async function play(page: Page, ms: number) {
  await page.getByRole('button', { name: 'Play', exact: true }).click()
  await page.waitForTimeout(ms)
}

async function screens(browser: Browser, state: string, demo: Demo, capture: string) {
  const context = await pinnedContext(browser, { storageState: state })
  const page = await context.newPage()
  const site = `/sites/${demo.site.id}`

  await page.goto(site)
  await page.getByText('Noticed changes').waitFor()
  await shot(page, 'overview')

  await page.goto(`${site}/realtime`)
  await page.waitForTimeout(3000)
  await shot(page, 'realtime')

  await page.goto(`${site}/sessions`)
  await shot(page, 'sessions')

  await page.goto(`${site}/replays/${capture}`)
  await loaded(page)
  await play(page, 3500)
  await page.getByRole('button', { name: 'Pause', exact: true }).click()
  await shot(page, 'replay')

  await page.goto(`${site}/journeys`)
  await shot(page, 'explore')
  await context.close()
}

/** Records the walk from the team's sites to an overview, a finding's replays and one replay, as a GIF. */
async function walkthrough(browser: Browser, state: string, demo: Demo, capture: string) {
  const videos = mkdtempSync(join(tmpdir(), 'holocron-video-'))
  const size = { width: 1440, height: 900 }
  const context = await pinnedContext(browser, { deviceScaleFactor: 1, storageState: state, recordVideo: { dir: videos, size } })
  const page = await context.newPage()
  await page.goto(`/workspaces/${demo.workspace.id}`)
  await loaded(page)
  await settle(page)
  await page.waitForTimeout(1800)

  await page.locator(`main a[href="/sites/${demo.site.id}"]`).first().click()
  await page.getByText('Noticed changes').waitFor()
  await loaded(page)
  await page.waitForTimeout(2500)

  // The finding opens Explore, filtered to the newsletter page.
  await page.getByText('/de/newsletter/herbst is up', { exact: false }).click()
  await page.waitForURL(/\/explore/)
  await loaded(page)
  await page.waitForTimeout(2500)

  await page.getByRole('link', { name: 'Replays', exact: true }).click()
  await loaded(page)
  await page.waitForTimeout(1500)

  await page.locator(`a[href$="/replays/${capture}"]`).first().click()
  await loaded(page)
  await page.waitForTimeout(800)
  await play(page, 7000)
  await context.close()

  const [video] = readdirSync(videos)
  if (!video) throw new Error('Playwright wrote no video')
  const gif = join(assets, 'walkthrough.gif')
  // The first second is the blank page before the first load.
  ffmpeg([
    '-ss', '1.2', '-i', join(videos, video),
    '-vf', 'fps=10,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle',
    gif,
  ])
  rmSync(videos, { recursive: true, force: true })
  console.log('wrote walkthrough.gif')
}

function ffmpeg(args: string[]) {
  const result = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', ...args], { stdio: 'inherit' })
  if (result.status !== 0) throw new Error(`ffmpeg ${args.join(' ')} exited with ${result.status}`)
}

/** Renders `scripts/social-preview.html`, which shows a crop of `overview.png`, at 1280x640. */
async function socialPreview(browser: Browser) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 640 } })
  await page.goto(pathToFileURL(join(repo, 'scripts', 'social-preview.html')).href)
  await settle(page)
  await page.screenshot({ path: socialCard })
  await page.close()
  console.log('wrote .github/social-preview.png')
}

/** Re-encodes every PNG with the strongest zlib setting; lossless. */
function compress() {
  const pngs = readdirSync(assets).filter((name) => name.endsWith('.png')).map((name) => join(assets, name))
  for (const path of [...pngs, socialCard]) {
    const packed = `${path}.tmp.png`
    ffmpeg(['-i', path, '-compression_level', '100', '-pred', 'mixed', packed])
    renameSync(packed, path)
  }
}

mkdirSync(assets, { recursive: true })
const browser = await chromium.launch()
try {
  const state = await signIn(browser)
  const context = await pinnedContext(browser, { storageState: state })
  const page = await context.newPage()
  await page.goto('/')
  const found = await lagoon(page)
  await liveVisitors(found.site)
  const capture = await recordedVisit(found.site)
  await waitForReplay(page, found.site, capture)
  await context.close()

  await screens(browser, state, found, capture)
  await socialPreview(browser)
  await walkthrough(browser, state, found, capture)
  rmSync(resolve(state, '..'), { recursive: true, force: true })
} finally {
  await browser.close()
}
compress()
