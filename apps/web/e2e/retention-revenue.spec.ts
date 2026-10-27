import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { admin, sessionFile } from './session'

/**
 * Milestone 08 journeys against a server seeded with `seed_demo`, `seed_traffic` and
 * `seed_commerce`: retention, revenue and performance, their labels through save, export, pin
 * and reopen, the empty state of a site without consent, phones, dark mode and the keyboard.
 * Set HOLOCRON_E2E_PASSWORD; HOLOCRON_E2E_SHOTS writes screenshots.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

async function signedIn(browser: Browser, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options, storageState: sessionFile(admin) })
  return context.newPage()
}

/** The site `seed_commerce` filled (picked by name), or one with anonymous traffic only. */
async function siteId(page: Page, which: 'commerce' | 'anonymous'): Promise<string> {
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> }
  const name = which === 'commerce' ? /^Roadhouse Konzertagentur/ : /^roadhouse\.test$/
  const site = workspaces.flatMap((w) => w.sites).find((s) => name.test(s.name))
  if (!site) throw new Error('seed_demo did not run')
  return site.id
}

async function shot(page: Page, name: string, fullPage = true) {
  if (!shots) return
  await expect(page.getByText('Showing the previous result while this one loads.')).toHaveCount(0)
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))))
  await page.screenshot({ path: join(shots, `${name}.png`), fullPage })
}

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const page = document.documentElement.scrollWidth - window.innerWidth
    const clipped = Array.from(document.querySelectorAll('#main-content *')).filter((element) => {
      const box = element.getBoundingClientRect()
      return (
        box.width > 0 &&
        box.right > window.innerWidth + 1 &&
        !element.closest('.sr-only') &&
        !element.closest('[role=region]') &&
        !element.closest('.overflow-x-auto')
      )
    })
    return { page, clipped: clipped.slice(0, 3).map((element) => element.outerHTML.slice(0, 120)) }
  })
  expect(overflow).toEqual({ page: 0, clipped: [] })
}

function urlState(page: Page): unknown {
  const s = new URL(page.url()).searchParams.get('s')
  return s ? JSON.parse(Buffer.from(s, 'base64url').toString('utf8')) : null
}

/** Every width and both modes of one screen, without page overflow. */
async function everyLayout(page: Page, name: string) {
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 })
      await expectNoHorizontalOverflow(page)
      await shot(page, `${name}-${width === 390 ? 'phone' : 'desktop'}-${scheme}`)
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.emulateMedia({ colorScheme: 'light' })
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('retention: labels, cohort table and keyboard, kept through save, export, pin and reopen', async ({ browser }) => {
  test.setTimeout(120_000)
  const page = await signedIn(browser, { colorScheme: 'light' })
  const site = await siteId(page, 'commerce')
  await page.goto(`/sites/${site}/retention`)
  await expect(page.getByRole('heading', { level: 1, name: 'Retention' })).toBeVisible()
  // The table and its population notes live in the details view; the heatmap opens first.
  await page.getByRole('radio', { name: 'Details' }).click()

  const labels = page.getByRole('list', { name: 'Population and limits of this report' })
  await expect(labels).toContainText('Consented persistent profiles only.')
  await expect(labels).toContainText('came from consented profiles')
  await expect(labels).toContainText('start of retained history')
  const table = page.getByRole('region', { name: 'Cohort table' })
  await expect(table.getByRole('columnheader', { name: 'Week 0' })).toBeVisible()
  await expect(table.getByRole('rowheader', { name: 'Average' })).toBeVisible()
  await expect(table.getByText('(incomplete)').first()).toBeAttached()
  await expect(page.getByRole('button', { name: 'Compare with the previous period' })).toHaveCount(0)
  await everyLayout(page, 'retention-weekly')

  // Keyboard: the cohort period is a radio group reached and switched without a pointer.
  await page.getByRole('radio', { name: 'Weekly cohorts' }).focus()
  await page.keyboard.press('Shift+Tab')
  await page.keyboard.press('Tab')
  await expect(page.getByRole('radio', { name: 'Weekly cohorts' })).toBeFocused()
  await page.getByRole('radio', { name: 'Daily cohorts' }).focus()
  await page.keyboard.press('Enter')
  await expect.poll(() => (urlState(page) as { granularity: string }).granularity).toBe('day')
  await page.getByRole('button', { name: 'Intervals: 14 days' }).click()
  await page.getByRole('menuitemradio', { name: '7 days' }).click()
  await expect(table.getByRole('columnheader', { name: 'Day 6' })).toBeVisible()
  const state = urlState(page) as { report: { kind: string; v: number; intervals: number } }
  expect(state.report).toEqual({ kind: 'retention', v: 1, intervals: 7 })
  await shot(page, 'retention-daily-desktop-light')

  // Save the view, export the state with its labelled result, pin it and reopen it.
  const viewName = `Daily retention ${Date.now()}`
  await page.getByRole('button', { name: /^Saved view/ }).click()
  await page.getByRole('menuitem', { name: 'Save this view' }).click()
  await page.getByRole('textbox', { name: 'Name this view' }).fill(viewName)
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: `Saved view: ${viewName}` })).toBeVisible()

  const download = page.waitForEvent('download')
  await page.getByRole('button', { name: 'Export' }).click()
  await page.getByRole('menuitem', { name: 'Download JSON with state' }).click()
  const file = JSON.parse(readFileSync(await (await download).path(), 'utf8')) as {
    state: unknown
    result: { report: { kind: string; coverage: unknown; history_start: number; watermark: number } }
  }
  expect(file.state).toEqual(state)
  expect(file.result.report.kind).toBe('retention')
  expect(file.result.report.coverage).toBeTruthy()
  expect(file.result.report.history_start).toBeGreaterThan(0)

  await page.getByRole('button', { name: 'Pin to board' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Board', { exact: true }).selectOption('')
  const boardName = `Retention ${Date.now()}`
  await dialog.getByLabel('New board name').fill(boardName)
  await dialog.getByLabel('Card title').fill('Daily retention')
  await dialog.getByRole('button', { name: 'Pin' }).click()
  await expect(page.getByText(`Pinned to ${boardName}.`)).toBeVisible()
  await page.getByRole('navigation', { name: 'Analytics navigation' }).getByRole('link', { name: 'Boards' }).click()
  await page.getByRole('link', { name: new RegExp(boardName) }).click()
  await expect(page.getByRole('heading', { level: 2, name: 'Daily retention' })).toBeVisible()
  await expect(page.getByText(/^Consented persistent profiles only:/)).toBeVisible()
  await shot(page, 'retention-board-card-light')
  await page.getByRole('button', { name: 'Options for Daily retention' }).click()
  await page.getByRole('menuitem', { name: 'Open report' }).click()
  await page.waitForURL(/\/retention\?/)
  await expect(page.getByRole('heading', { level: 1, name: 'Retention' })).toBeVisible()
  expect(urlState(page)).toEqual(state)
  await expect(page.getByRole('button', { name: `Saved view: ${viewName}` })).toBeVisible()
  await page.context().close()
})

test('retention without consent explains why instead of showing 0%', async ({ browser }) => {
  const page = await signedIn(browser, { colorScheme: 'dark' })
  await page.goto(`/sites/${await siteId(page, 'anonymous')}/retention`)
  await expect(page.getByText('No consented profiles in these periods')).toBeVisible()
  await expect(page.getByText(/new pseudonym every day/).first()).toBeVisible()
  await expect(page.getByRole('link', { name: 'Turn on persistent identity' })).toBeVisible()
  await expect(page.getByRole('region', { name: 'Cohort table' })).toHaveCount(0)
  await everyLayout(page, 'retention-no-consent')
  await page.context().close()
})

test('revenue: currencies apart, exact amounts, breakdown and export', async ({ browser }) => {
  const page = await signedIn(browser, { colorScheme: 'light' })
  const site = await siteId(page, 'commerce')
  await page.goto(`/sites/${site}/revenue`)
  await expect(page.getByRole('heading', { level: 1, name: 'Revenue' })).toBeVisible()
  // The summary opens first: pace and the sources that pay; cards, chart and breakdown live in Details.
  await expect(page.getByRole('heading', { name: 'Which traffic pays' })).toBeVisible()
  await expect(page.getByRole('radiogroup', { name: 'Source' }).getByRole('radio', { name: 'Channel' })).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('radio', { name: 'Details' }).click()
  // seed_commerce puts its single JPY order anywhere in 45 days; the default 30 days can miss it.
  await page.getByRole('button', { name: /^Scope: / }).click()
  await page.getByRole('button', { name: /^Date range:/ }).click()
  await page.getByRole('menuitemradio', { name: 'Last 90 days' }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: /^Scope: / }).click()
  // Currencies stay apart: one switch option each, and the measures show one currency at a time.
  const currencies = page.getByRole('radiogroup', { name: 'Currency' })
  for (const currency of ['EUR', 'JPY', 'USD']) await expect(currencies.getByRole('radio', { name: currency })).toBeVisible()
  await expect(page.getByRole('region', { name: 'Summary measures' }).getByRole('button', { name: /^Net revenue €/ })).toBeVisible()
  await expect(page.getByLabel(/never converted/)).toBeVisible()
  const results = page.getByRole('region', { name: 'Results' })
  // The table lists the selected currency only: header, its one channel and the total.
  await expect(results.getByRole('row')).toHaveCount(3)
  await expect(results.getByRole('row').nth(1)).toContainText('€')
  await page.getByRole('radio', { name: 'USD' }).click()
  await expect(page.getByRole('figure', { name: /^Net revenue, USD over/ })).toBeVisible()
  await everyLayout(page, 'revenue')

  await page.getByRole('button', { name: 'Breakdown: By currency' }).click()
  await page.getByRole('menuitemradio', { name: 'Country' }).click()
  await expect.poll(() => (urlState(page) as { dimensions: string[] }).dimensions).toEqual(['currency', 'country'])
  await expect(results.getByRole('row').nth(1)).toContainText('Austria')
  await shot(page, 'revenue-by-country-desktop-light')

  const download = page.waitForEvent('download')
  await page.getByRole('button', { name: 'Export' }).click()
  await page.getByRole('menuitem', { name: 'Download CSV' }).click()
  const csv = readFileSync(await (await download).path(), 'utf8')
  // The default comparison adds previous and change columns after each measure.
  const header = csv.split('\r\n')[0]?.split(',') ?? []
  expect(header.slice(0, 2)).toEqual(['currency', 'country'])
  expect(header).toEqual(expect.arrayContaining(['revenue_net', 'revenue_gross', 'revenue_refunds', 'revenue_count']))
  expect(csv).toMatch(/\d+\.\d{2} EUR/)
  await page.context().close()
})

test('performance: named vitals with units, ratings in words, breakdown by device', async ({ browser }) => {
  const page = await signedIn(browser, { colorScheme: 'light' })
  const site = await siteId(page, 'commerce')
  await page.goto(`/sites/${site}/performance`)
  await expect(page.getByRole('heading', { level: 1, name: 'Performance' })).toBeVisible()
  const cards = page.getByRole('region', { name: 'Web vitals at the 75th percentile' })
  await expect(cards.getByText('Largest contentful paint')).toBeVisible()
  await expect(cards.getByText(/ms$/).first()).toBeVisible()
  const table = page.getByRole('region', { name: 'Percentiles per metric' })
  await expect(table.getByRole('row')).toHaveCount(6)
  await expect(table.getByRole('rowheader', { name: 'Cumulative layout shift' })).toBeVisible()
  await expect(table.getByText(/^(Good|Needs improvement|Poor)$/).first()).toBeVisible()
  await everyLayout(page, 'performance')

  await page.getByRole('button', { name: 'Breakdown: By metric' }).click()
  await page.getByRole('menuitemradio', { name: 'Device' }).click()
  await expect(table.getByRole('columnheader', { name: 'Device' })).toBeVisible()
  await expect(table.getByText('mobile').first()).toBeVisible()
  await expect(cards.getByText('Largest contentful paint')).toBeVisible()
  await shot(page, 'performance-by-device-desktop-light')
  await page.context().close()
})
