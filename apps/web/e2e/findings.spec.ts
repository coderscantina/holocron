import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type Page } from '@playwright/test'
import { admin, sessionFile } from './session'

/**
 * The overview's findings against a server seeded with `seed_demo` and `seed_traffic`. Set
 * HOLOCRON_E2E_PASSWORD; HOLOCRON_E2E_SHOTS writes screenshots. Seeded traffic may not trigger
 * any finding, so the actions run against a mocked `summaries.findings` answer.
 */
const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

async function signedIn(browser: Browser, viewport = { width: 1440, height: 900 }) {
  const context = await browser.newContext({ viewport, storageState: sessionFile(admin) })
  const page = await context.newPage()
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  return page
}

async function roadhouse(page: Page): Promise<string> {
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> }
  const site = workspaces.flatMap((w) => w.sites).find((s) => s.name === 'roadhouse.test')
  if (!site) throw new Error('seed_demo did not run')
  return site.id
}

function mockedFindings(site: string, only?: 'empty') {
  const now = Date.now()
  const state = {
    v: 1,
    site,
    measures: ['visitor_days'],
    dimensions: [],
    filter: null,
    range: { kind: 'relative', unit: 'day', count: 1 },
    timezone: 'Europe/Vienna',
    granularity: 'hour',
    compare: { kind: 'typical_weekday' },
    metric_version: 1,
    sort: [],
    page: { size: 25, index: 0 },
    visualization: 'line',
  }
  const base = { computed_at: now, window_start: now - 6 * 3_600_000, share: 0, rule_version: 1, state }
  const cell = (v: string) => ({ t: 'string', v })
  return {
    site,
    computed_at: now,
    watermark_batch: 1,
    watermark_sequence: 1,
    stale: false,
    findings: only === 'empty' ? [] : [
      {
        ...base,
        rank: 1,
        rule: 'dimension_mover',
        share: 0.64,
        state: {
          ...state,
          filter: {
            kind: 'group',
            op: 'and',
            items: [{ kind: 'cond', id: 'f1', field: 'referrer_domain', cmp: 'eq', value: cell('instagram.com') }],
          },
        },
        values: {
          kind: 'dimension_mover',
          dimension: 'referrer_domain',
          value: cell('instagram.com'),
          today: 312,
          typical: 98,
          ratio: 3.18,
          site_today: 900,
          site_typical: 600,
        },
      },
      {
        ...base,
        rank: 2,
        rule: 'funnel_step',
        values: {
          kind: 'funnel_step',
          card: 'card1',
          title: 'Ticket checkout',
          step: 2,
          until_hour: 14,
          entered: 214,
          dropped: 86,
          baseline_entered: 1000,
          baseline_dropped: 310,
          baseline_days: 8,
          drop_off: 0.4,
          baseline_drop_off: 0.31,
        },
      },
      {
        ...base,
        rank: 3,
        rule: 'performance',
        values: {
          kind: 'performance',
          metric: 'lcp',
          device: cell('mobile'),
          value: { t: 'millis', v: 3400 },
          observations: 1420,
          slowest: { t: 'millis', v: 2600 },
          slowest_day: '2026-10-01',
          days: 28,
        },
      },
      { ...base, rank: 4, rule: 'seasonality', rule_version: 3, values: { kind: 'seasonality' } },
    ],
  }
}

async function mockFindings(page: Page, site: string, only?: 'empty') {
  await page.route('**/api/queries/summaries.findings*', (route) => route.fulfill({ json: mockedFindings(site, only) }))
  await page.route('**/api/queries/recordings.policy*', (route) =>
    route.fulfill({ json: { policy: { replay_enabled: true, heatmap_enabled: false, version: 1 } } })
  )
  await page.route('**/api/queries/recordings.list*', (route) => route.fulfill({ json: { captures: [], total: 12 } }))
}

/** Switches the overview from its today view to the details view, as a person does with the toggle. */
async function details(page: Page) {
  const radio = page.getByRole('radiogroup', { name: 'Overview view' }).getByRole('radio', { name: 'Details' })
  await radio.click()
  await expect(radio).toHaveAttribute('aria-checked', 'true')
}

async function shot(page: Page, name: string) {
  if (!shots) return
  await page.evaluate(() =>
    Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Infinity).map((a) => a.finished.catch(() => undefined)))
  )
  await page.getByRole('region', { name: 'Noticed changes' }).screenshot({ path: join(shots, `${name}.png`) })
}

test('the overview shows its findings or says nothing is unusual', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}`)
  const today = page.getByRole('region', { name: 'Today', exact: true })
  await expect(today.getByRole('heading', { name: 'Noticed changes' })).toBeVisible()
  await expect(today.getByRole('link').or(today.getByText(/Nothing unusual so far today|first summary|catching up/)).first()).toBeVisible()
  await details(page)
  const region = page.getByRole('region', { name: 'Noticed changes' })
  await expect(region).toBeVisible()
  await expect(region.getByRole('group').or(region.getByText(/Nothing unusual so far today|first summary|catching up/)).first()).toBeVisible()
  await shot(page, 'findings-live')
})

test('Filter to this adds one history entry, and the replay link carries the same filter', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await mockFindings(page, site)
  await page.goto(`/sites/${site}`)
  await details(page)
  const banner = page.getByRole('group', { name: /instagram\.com is up 218%/ })
  await expect(banner).toBeVisible()
  await expect(page.getByRole('region', { name: 'Noticed changes' }).getByRole('group')).toHaveCount(3)
  await shot(page, 'findings-mocked')

  const replays = banner.getByRole('link', { name: /Watch 12 replays/ })
  await expect(replays).toHaveAttribute('href', /\/replays\?range=1d&filter=/)

  const before = await page.evaluate(() => history.length)
  await banner.getByRole('button', { name: /^Filter to this: / }).click()
  await expect.poll(() => page.url()).toContain('s=')
  expect(await page.evaluate(() => history.length)).toBe(before + 1)
  await expect(page.getByRole('region', { name: 'Noticed changes' }).getByText('For all traffic')).toBeVisible()

  await page.getByRole('button', { name: 'Show all 4' }).click()
  const sheet = page.getByRole('dialog', { name: 'All findings' })
  await expect(sheet.getByText('Something changed in this site')).toBeVisible()
  await expect(sheet.getByRole('link', { name: /^Open details: / })).toBeVisible()
})

test('a site without findings says nothing is unusual', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await mockFindings(page, site, 'empty')
  await page.goto(`/sites/${site}`)
  await expect(page.getByRole('region', { name: 'Today', exact: true }).getByText('Nothing unusual so far today.')).toBeVisible()
  await details(page)
  await expect(page.getByRole('region', { name: 'Noticed changes' }).getByText('Nothing unusual so far today.')).toBeVisible()
  await shot(page, 'findings-empty')
})

test('the line under the headline names the mover; its value adds the filter with one history entry', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await mockFindings(page, site)
  await page.goto(`/sites/${site}`)
  await details(page)
  const summary = page.getByRole('region', { name: 'Summary' })
  const line = summary.locator('p', { hasText: /^Most of the lift is instagram\.com: \+214 visitor-days since \d/ })
  await expect(line).toBeVisible()

  if (shots) {
    for (const scheme of ['dark', 'light'] as const) {
      await page.emulateMedia({ colorScheme: scheme })
      for (const width of [1440, 390]) {
        await page.setViewportSize({ width, height: 900 })
        await page.evaluate(() =>
          Promise.all(document.getAnimations().filter((a) => a.effect?.getTiming().iterations !== Infinity).map((a) => a.finished.catch(() => undefined)))
        )
        await page.screenshot({ path: join(shots, `why-${width}-${scheme}.png`) })
      }
    }
    await page.setViewportSize({ width: 1440, height: 900 })
  }

  const before = await page.evaluate(() => history.length)
  await line.getByRole('button', { name: 'Filter to instagram.com' }).click()
  await expect.poll(() => page.url()).toContain('s=')
  expect(await page.evaluate(() => history.length)).toBe(before + 1)
  await expect(page.getByRole('region', { name: 'Noticed changes' }).getByText('For all traffic')).toBeVisible()
  await expect(summary.getByText(/Most of the lift/)).toHaveCount(0)
})
