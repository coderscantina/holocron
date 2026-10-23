import { mkdirSync, readdirSync, readFileSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { type Account, admin, viewer, sessionFile } from './session'

/**
 * Replay and heatmaps against the packaged server and the `seed_demo` data, with the unchanged
 * recorder on a shop page served by this test from another loopback port (Chromium's local
 * network rules refuse a public origin loading scripts from loopback). Needs HOLOCRON_E2E_PASSWORD and
 * HOLOCRON_E2E_VIEWER_PASSWORD; HOLOCRON_E2E_SHOTS writes screenshots there.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

/** isbot flags headless Chromium's own user agent; the recorder would be answered as a bot. */
const DESKTOP_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const SECRET_TEXT = 'Customer Jane Roe 4711'
const SECRET_ELEMENT = 'IBAN AT61 1904 3002'
const SECRET_INPUT = 's3cret-typed-value'
const HOSTILE_HOST = 'beacon.hostile.test'

test.describe.configure({ mode: 'serial' })

let shop: Server
let shopOrigin = ''
let shopHtml = ''

test.beforeAll(async () => {
  shop = createServer((_, response) => response.writeHead(200, { 'content-type': 'text/html' }).end(shopHtml))
  await new Promise<void>((resolve) => shop.listen(0, '127.0.0.1', resolve))
  const address: AddressInfo | string | null = shop.address()
  if (address === null || typeof address === 'string') throw new Error('shop server has no port')
  shopOrigin = `http://127.0.0.1:${address.port}`
})

test.afterAll(() => new Promise<void>((resolve) => shop.close(() => resolve())))

async function settle(page: Page) {
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        // Scroll-driven animations never finish; only time-based ones settle.
        .filter((animation) => animation.timeline instanceof DocumentTimeline)
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined))
    )
  )
}

async function shot(page: Page, name: string) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`) })
}

async function signedIn(browser: Browser, account: Account = admin, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options, storageState: sessionFile(account) })
  return context.newPage()
}

/** The ID of the seeded roadhouse.test site, from the caller's own workspace list. */
async function siteId(page: Page): Promise<string> {
  await page.goto('/')
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const body: { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> } = await answer.json()
  const site = body.workspaces.flatMap((workspace) => workspace.sites).find((entry) => entry.name === 'roadhouse.test')
  if (!site) throw new Error('roadhouse.test is not seeded')
  return site.id
}

/** A shop page carrying the unchanged tracker and recorder for `site`. */
function shopPage(baseURL: string, site: string) {
  return `<!doctype html><html><head><title>Shop</title></head><body>
    <main style="height:2400px;font:16px sans-serif">
      <h1>${SECRET_TEXT}</h1>
      <p class="private">${SECRET_ELEMENT}</p>
      <label>Note <input id="note" type="text"></label>
      <button id="buy" style="margin:40px">Buy</button>
    </main>
    <script defer src="${baseURL}/script.js" data-website-id="${site}"></script>
    <script defer src="${baseURL}/recorder.js" data-website-id="${site}"></script>
  </body></html>`
}

/** Opens the shop in a fresh visitor context and returns every `/api/record` body it sent. */
async function visitShop(browser: Browser, baseURL: string, site: string, act: (page: Page) => Promise<void>) {
  shopHtml = shopPage(baseURL, site)
  const context = await browser.newContext({ userAgent: DESKTOP_UA, viewport: { width: 1280, height: 720 } })
  const page = await context.newPage()
  const bodies: string[] = []
  const answers: Array<Promise<string>> = []
  page.on('request', (request) => {
    if (request.url().endsWith('/api/record')) bodies.push(request.postData() ?? '')
  })
  page.on('response', (response) => {
    if (response.url().endsWith('/api/record')) answers.push(response.text())
  })
  await page.goto(`${shopOrigin}/`)
  await page.waitForFunction(() => 'umami' in window)
  await page.waitForTimeout(2500)
  await act(page)
  await page.waitForTimeout(6500)
  await context.close()
  return { bodies, answers: await Promise.all(answers) }
}

test('recording settings turn replay and heatmaps on with exclusions', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await siteId(page)
  await page.goto(`/sites/${site}/settings/recording`)
  await expect(page.getByText('Text and form inputs are always masked in the browser before they are sent.')).toBeVisible()
  await page.getByRole('switch', { name: 'Session replays' }).check()
  await page.getByRole('switch', { name: 'Heatmaps' }).check()
  await page.getByLabel('Replay sampling, %').fill('100')
  await page.getByLabel('Heatmap sampling, %').fill('100')
  await page.getByLabel('Elements never recorded').fill('.private')
  await page.getByLabel('Pages never recorded').fill('/account*')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.getByRole('status').getByText('Saved.')).toBeVisible()
  const config = await (await page.request.get(`/api/websites/${site}/recorder`)).json()
  expect(config).toMatchObject({ enabled: true, maskLevel: 'strict', sampleRate: 1, blockSelector: '.private' })
  await shot(page, 'settings-1440-light')
})

test('the unchanged recorder masks and excludes before anything is sent', async ({ browser, baseURL }) => {
  const page = await signedIn(browser)
  const site = await siteId(page)
  const visit = await visitShop(browser, baseURL ?? '', site, async (shop) => {
    await shop.locator('#note').pressSequentially(SECRET_INPUT)
    await shop.locator('#buy').click()
    await shop.mouse.wheel(0, 1200)
  })
  expect(visit.bodies.some((body) => body.includes('"type":"record"'))).toBe(true)
  expect(visit.bodies.some((body) => body.includes('"type":"heatmap"'))).toBe(true)
  for (const secret of [SECRET_TEXT, SECRET_ELEMENT, SECRET_INPUT]) {
    expect(visit.bodies.some((body) => body.includes(secret)), secret).toBe(false)
  }
  expect(visit.answers.every((answer) => answer === '{"ok":true}')).toBe(true)

  // A second visitor reaches an excluded page: the server drops that replay whole.
  const excluded = await visitShop(browser, baseURL ?? '', site, async (shop) => {
    await shop.evaluate(() => history.pushState({}, '', '/account/orders'))
    await shop.locator('#buy').click()
  })
  expect(excluded.answers).toContain('{"ok":false,"reason":"page_excluded"}')

  await page.goto(`/sites/${site}/replays`)
  await expect(page.getByRole('link', { name: /^Play replay recorded/ })).toHaveCount(1)
  await shot(page, 'replays-1440-light')
})

test('a replay plays in the isolated frame with controls and a timeline', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await siteId(page)
  await page.goto(`/sites/${site}/replays`)
  await page.getByRole('link', { name: /^Play replay recorded/ }).first().click()
  const play = page.getByRole('button', { name: 'Play', exact: true })
  await expect(play).toBeEnabled({ timeout: 15_000 })

  const replayed = page.frameLocator('iframe[title^="Replay of a recorded visit"]').frameLocator('iframe')
  await expect(replayed.locator('h1')).toHaveText(/^[* ]+$/)
  await expect(replayed.locator('body')).not.toContainText('Jane')

  const timeline = page.getByRole('list').filter({ has: page.getByRole('button', { name: /^Click,/ }) })
  await expect(timeline.getByRole('button', { name: /^Page \// }).first()).toBeVisible()
  await play.click()
  await page.getByRole('button', { name: 'Pause', exact: true }).click()
  await expect(play).toBeVisible()
  await page.getByRole('button', { name: /^Click,/ }).first().click()
  await page.getByRole('radio', { name: '4×' }).click()
  await expect(page.getByRole('radio', { name: '4×' })).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('button', { name: 'Save replay' }).click()
  await expect(page.getByRole('button', { name: 'Remove from saved' })).toBeVisible()
  await shot(page, 'replay-1440-light')
})

test('hostile captured documents run no script and reach no network', async ({ browser, baseURL }) => {
  test.setTimeout(120_000)
  const page = await signedIn(browser)
  const site = await siteId(page)
  const dir = join(import.meta.dirname, '../../../fixtures/replay-hostile')
  const cases = readdirSync(dir).filter((name) => name.endsWith('.json'))
  const visitor = await browser.newContext({ userAgent: DESKTOP_UA })
  for (const [index, name] of cases.entries()) {
    const fixture: { events: unknown[] } = JSON.parse(readFileSync(join(dir, name), 'utf8'))
    const sent = await visitor.request.post(`${baseURL}/api/send`, {
      data: { type: 'event', payload: { website: site, url: `/hostile/${name}`, hostname: 'shop.test' } },
    })
    const { cache } = await sent.json()
    const recorded = await visitor.request.post(`${baseURL}/api/record`, {
      headers: { 'x-umami-cache': cache },
      data: { type: 'record', payload: { website: site, events: fixture.events, timestamp: 1_800_000_000 + index } },
    })
    expect(await recorded.json(), name).toEqual({ ok: true })
  }
  await visitor.close()

  const reached: string[] = []
  await page.context().route(`https://${HOSTILE_HOST}/**`, (route) => {
    reached.push(route.request().url())
    return route.fulfill({ status: 204 })
  })
  const dialogs: string[] = []
  page.on('dialog', (dialog) => {
    dialogs.push(dialog.message())
    return dialog.dismiss()
  })
  await page.goto(`/sites/${site}/replays?path=/hostile`)
  const links = page.getByRole('link', { name: /^Play replay recorded/ })
  await expect(links).toHaveCount(cases.length)
  for (let index = 0; index < cases.length; index += 1) {
    await page.goto(`/sites/${site}/replays?path=/hostile`)
    await links.nth(index).click()
    const play = page.getByRole('button', { name: 'Play', exact: true })
    await expect(play).toBeEnabled({ timeout: 15_000 })
    await play.click()
    await page.waitForTimeout(2500)
    const replayed = page.frameLocator('iframe[title^="Replay of a recorded visit"]').frameLocator('iframe')
    const html = await replayed.locator('html').innerHTML()
    expect(html).not.toMatch(/<script|onerror=|javascript:|srcdoc=/i)
    expect(await page.title()).not.toContain('pwned')
  }
  expect(reached).toEqual([])
  expect(dialogs).toEqual([])
})

/** A filter tree in the URL form of decision 0004: base64url of its canonical JSON. */
function filterParam(filter: unknown) {
  const canonical = (value: unknown): string => {
    if (value === null || typeof value !== 'object') return JSON.stringify(value)
    if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
    const entries = Object.entries(value).sort(([a], [b]) => (a < b ? -1 : 1))
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonical(item)}`).join(',')}}`
  }
  return Buffer.from(canonical(filter)).toString('base64url')
}

const pathIs = (id: string, path: string) => ({ kind: 'cond', id, field: 'path', cmp: 'eq', value: { t: 'string', v: path } })

test('a filter link lists exactly the replays whose session matches', async ({ browser, baseURL }) => {
  const page = await signedIn(browser)
  const site = await siteId(page)
  // Two visitors (their user agents differ), each with one pageview and one replay.
  for (const [index, path] of ['/finding/match', '/finding/other'].entries()) {
    const visitor = await browser.newContext({ userAgent: DESKTOP_UA.replace('140.0.0.0', `141.0.0.${index}`) })
    const sent = await visitor.request.post(`${baseURL}/api/send`, {
      data: { type: 'event', payload: { website: site, url: path, hostname: 'shop.test' } },
    })
    const { cache } = await sent.json()
    const events = [
      { type: 4, timestamp: Date.now(), data: { href: `https://shop.test${path}`, width: 1280, height: 720 } },
      { type: 3, timestamp: Date.now() + 10, data: { source: 2, type: 2, id: 4, x: 5, y: 5 } },
    ]
    const recorded = await visitor.request.post(`${baseURL}/api/record`, {
      headers: { 'x-umami-cache': cache },
      data: { type: 'record', payload: { website: site, events, timestamp: 1 } },
    })
    expect(await recorded.json()).toEqual({ ok: true })
    await visitor.close()
  }

  const finding = { kind: 'group', op: 'and', items: [pathIs('f1', '/finding/match')] }
  const now = Date.now()
  const listed = async () => {
    const args = new URLSearchParams({
      site,
      start: String(now - 86_400_000),
      end: String(now + 60_000),
      filter: JSON.stringify(finding),
    })
    const answer = await page.request.get(`/api/queries/recordings.list?${args}`, { headers: { 'x-holocron-request': '1' } })
    const body: { total?: number } = await answer.json()
    return body.total
  }
  // Sessions are derived in the background; the list links captures once they exist.
  await expect.poll(listed, { timeout: 20_000 }).toBe(1)

  await page.goto(`/sites/${site}/replays?filter=${filterParam(finding)}`)
  await expect(page.getByText('Replays whose session matches Page is /finding/match')).toBeVisible()
  await expect(page.getByRole('link', { name: /^Play replay recorded/ })).toHaveCount(1)
  await shot(page, 'replays-filtered-1440-light')
  await page.getByRole('link', { name: 'Show all replays' }).click()
  await expect(page).not.toHaveURL(/filter=/)

  const tooMany = filterParam({
    kind: 'group',
    op: 'or',
    items: [0, 17].map((from) => ({
      kind: 'group',
      op: 'or',
      items: Array.from({ length: 17 }, (_, i) => pathIs(`f${from + i}`, '/finding/match')),
    })),
  })
  await page.goto(`/sites/${site}/replays?filter=${tooMany}`)
  await expect(page.getByText('A view can hold 32 filter conditions at most.')).toBeVisible()
  await expect(page.getByRole('link', { name: /^Play replay recorded/ })).toHaveCount(0)

  await page.goto(`/sites/${site}/replays?filter=not-a-filter`)
  await expect(page.getByText('This replay filter cannot be read. The link may be cut off.')).toBeVisible()
  await expect(page.getByRole('link', { name: /^Play replay recorded/ })).toHaveCount(0)
})

test('heatmaps show clicks and scroll depth of the recorded page', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await siteId(page)
  await page.goto(`/sites/${site}/heatmaps`)
  const shopPage = page.getByRole('button', { name: /^\/, Clicks/ })
  await expect(shopPage).toBeVisible()
  await expect(page.getByRole('img', { name: 'Click density on /' })).toBeVisible()
  await expect(page.getByRole('heading', { name: '/account/orders' })).toHaveCount(0)
  await shot(page, 'heatmap-clicks-1440-light')
  await page.getByRole('tab', { name: 'Scroll depth' }).click()
  await expect(page.getByRole('rowheader', { name: '100%' })).toBeVisible()
  await shot(page, 'heatmap-scroll-1440-light')
})

test('viewers read replays but not the recording policy', async ({ browser }) => {
  const page = await signedIn(browser, viewer)
  const site = await siteId(page)
  await page.goto(`/sites/${site}/replays`)
  await expect(page.getByRole('link', { name: /^Play replay recorded/ }).first()).toBeVisible()
  await expect(page.getByRole('link', { name: 'Recording' })).toHaveCount(0)
  await page.goto(`/sites/${site}/settings/recording`)
  await expect(page.getByText('Your role does not allow this.')).toBeVisible()
})

for (const mode of ['light', 'dark'] as const) {
  for (const width of [1440, 390] as const) {
    test(`recording screens at ${width} px in ${mode} mode`, async ({ browser }) => {
      const page = await signedIn(browser, admin, { viewport: { width, height: 900 }, colorScheme: mode })
      const site = await siteId(page)
      for (const [path, name] of [
        ['replays', 'replays'],
        ['heatmaps', 'heatmaps'],
        ['settings/recording', 'settings'],
      ] as const) {
        await page.goto(`/sites/${site}/${path}`)
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
        expect(overflow, `${name} overflows`).toBeLessThanOrEqual(0)
        await shot(page, `${name}-${width}-${mode}`)
      }
      await page.goto(`/sites/${site}/replays`)
      await page.getByRole('link', { name: /^Play replay recorded/ }).last().click()
      await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 15_000 })
      await shot(page, `replay-${width}-${mode}`)
    })
  }
}
