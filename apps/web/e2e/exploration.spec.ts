import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { type Account, admin, viewer, sessionFile } from './session'

/**
 * Milestone 06 journeys against a server seeded with `seed_demo` and `seed_traffic`. Set
 * HOLOCRON_E2E_PASSWORD and HOLOCRON_E2E_VIEWER_PASSWORD; HOLOCRON_E2E_DB names the server's
 * `control.sqlite` for the revoked-scope journey; HOLOCRON_E2E_SHOTS writes screenshots.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

async function settle(page: Page) {
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
        .map((animation) => animation.finished.catch(() => undefined))
    )
  )
}

async function shot(page: Page, name: string, fullPage = false) {
  if (!shots) return
  await expect(page.getByText('Showing the previous result while this one loads.')).toHaveCount(0)
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`), fullPage })
}

async function signedIn(browser: Browser, account: Account = admin, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options, storageState: sessionFile(account) })
  return context.newPage()
}

/** The site ID of roadhouse.test, from the header options the server authorizes. */
async function roadhouse(page: Page): Promise<string> {
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> }
  const site = workspaces.flatMap((w) => w.sites).find((s) => s.name === 'roadhouse.test')
  if (!site) throw new Error('seed_demo did not run')
  return site.id
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

/** The exploration state in the address bar. */
function urlState(page: Page): unknown {
  const s = new URL(page.url()).searchParams.get('s')
  if (!s) return null
  return JSON.parse(Buffer.from(s, 'base64url').toString('utf8'))
}

/** Switches the overview from its today view to the details view, as a person does with the toggle. */
async function details(page: Page) {
  const details = page.getByRole('radiogroup', { name: 'Overview view' }).getByRole('radio', { name: 'Details' })
  await details.click()
  await expect(details).toHaveAttribute('aria-checked', 'true')
}

/** Shows the scope chips when the top bar has collapsed them into one summary chip. */
async function scope(page: Page) {
  const summary = page.getByRole('button', { name: /^Scope: / })
  await expect(page.getByRole('button', { name: /^(Scope: |Date range: )/ }).filter({ visible: true }).first()).toBeVisible()
  if (!(await summary.isVisible()) || (await summary.getAttribute('aria-expanded')) === 'true') return
  await summary.click()
  await expect(summary).toHaveAttribute('aria-expanded', 'true')
}

/** Opens the filter editor from the scope chips unless it is open. */
async function filterEditor(page: Page) {
  const editor = page.getByRole('dialog', { name: 'Filters and segments' })
  if (!(await editor.isVisible())) {
    await scope(page)
    await page.getByRole('button', { name: 'Filter or segment' }).click()
  }
  await expect(editor).toBeVisible()
  return editor
}

/** Builds one condition through the staged filter input, as a person types it. */
async function addFilter(page: Page, field: string, operator: string, value?: string) {
  const count = () => JSON.stringify(urlState(page) ?? {}).split('"kind":"cond"').length
  const before = count()
  const editor = await filterEditor(page)
  const input = editor.getByRole('combobox').first()
  await input.click()
  await input.fill(field)
  await page.getByRole('option', { name: field, exact: true }).click()
  await page.getByRole('option', { name: operator, exact: true }).click()
  if (value !== undefined) {
    await input.fill(value)
    await page.keyboard.press('Enter')
  }
  await expect.poll(count).toBe(before + 1)
  // The new chip can collapse the bar, which closes the editor; close it so the next step starts settled.
  if (await editor.isVisible()) await page.keyboard.press('Escape')
  await expect(editor).toBeHidden()
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('the overview shows real figures at every width in both modes', async ({ browser }) => {
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}`)
  await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible()
  await expect(page.getByText('Fixture data.')).toHaveCount(0)
  await expect(page.getByRole('region', { name: 'Today', exact: true }).getByText(/Pageviews today, until \d/)).toBeVisible()
  await details(page)
  const cards = page.getByRole('region', { name: 'Summary measures' })
  await expect(cards.getByRole('button', { name: /^Views \S/ })).toBeVisible()
  await expect(page.getByRole('table', { name: 'Pages' })).toContainText('/de/events')
  await expect(page.getByText('All figures corrected')).toBeVisible()

  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of [1440, 1024, 768, 390]) {
      await page.setViewportSize({ width, height: 900 })
      await expectNoHorizontalOverflow(page)
      await shot(page, `overview-${width}-${scheme}`)
    }
  }
  await page.context().close()
})

test('the overview opens on today against a typical weekday; chips and measures each take one step', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const site = await roadhouse(page)
  const explores: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('analytics.explore')) explores.push(request.url())
  })
  await page.goto(`/sites/${site}`)
  const stage = page.getByRole('region', { name: 'Today', exact: true })
  await expect(stage.getByText(/Pageviews today, until \d/)).toBeVisible()
  await expect(stage.getByText(/usual.* \w+day/).first()).toBeVisible()
  await expect(page.getByRole('group', { name: /^Pageviews today by the hour against a Usual \w+day\./ })).toBeVisible()
  await details(page)
  const headline = page.getByRole('region', { name: 'Summary', exact: true })
  await expect(headline).toContainText(/^[\d,]+ views so far today/)
  expect(urlState(page)).toBeNull()
  await scope(page)
  await expect(page.getByRole('button', { name: /^Date range: Today, until now, \d/ })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Comparison: vs a typical \w+day$/ })).toBeVisible()
  await shot(page, 'overview-default-1440-dark')
  await page.keyboard.press('Escape')

  // Picking a measure is one history entry and no request.
  const strip = page.getByRole('region', { name: 'Summary measures' })
  await expect(strip.getByRole('button', { name: /^Views / })).toHaveAttribute('aria-pressed', 'true')
  const before = explores.length
  await strip.getByRole('button', { name: /^Sessions / }).click()
  await expect(page).toHaveURL(/[?&]m=sessions/)
  await expect(strip.getByRole('button', { name: /^Sessions / })).toHaveAttribute('aria-pressed', 'true')
  await expect(headline).toContainText(/ sessions so far today/)
  await page.goBack()
  await expect(strip.getByRole('button', { name: /^Views / })).toHaveAttribute('aria-pressed', 'true')
  expect(explores.length).toBe(before)

  // The comparison chip: one state, one request; a 90-day range lists the typical weekday disabled.
  await scope(page)
  await page.getByRole('button', { name: /^Comparison:/ }).click()
  await page.getByRole('menuitemradio', { name: 'Previous period' }).click()
  await expect.poll(() => (urlState(page) as { compare: string } | null)?.compare).toBe('previous_period')
  await expect(headline).toContainText('the previous period')
  expect(explores.length).toBe(before + 1)
  await scope(page)
  await page.getByRole('button', { name: /^Date range:/ }).click()
  await page.getByRole('menuitemradio', { name: 'Last 90 days' }).click()
  await expect.poll(() => (urlState(page) as { range: { count: number } } | null)?.range.count).toBe(90)
  await scope(page)
  await page.getByRole('button', { name: /^Comparison:/ }).click()
  const typical = page.getByRole('menuitemradio', { name: /^Typical weekdays/ })
  await expect(typical).toHaveAttribute('data-disabled', '')
  await expect(typical).toContainText('Covers at most 31 days')
  await shot(page, 'overview-compare-blocked-dark')
  await page.keyboard.press('Escape')
  await page.goBack()
  await page.goBack()
  await expect.poll(() => urlState(page)).toBeNull()

  // A filter reads as its own chip and keeps the headline on the filtered state.
  await addFilter(page, 'Device', 'is', 'mobile')
  await expect(async () => {
    await scope(page)
    await expect(page.getByRole('button', { name: 'Edit the filter on Device' })).toBeVisible({ timeout: 1000 })
  }).toPass()
  await shot(page, 'overview-filtered-dark')
  await page.context().close()
})

test('the trend chart labels the estimate, the typical day and now; its interval control takes one step', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const site = await roadhouse(page)
  const explores: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('analytics.explore')) explores.push(request.url())
  })
  await page.goto(`/sites/${site}`)
  await details(page)
  const intervals = page.getByRole('radiogroup', { name: 'Interval' })
  const chart = page.locator('figure').filter({ has: intervals })
  const panel = chart.locator('..')
  await expect(intervals.getByRole('radio', { name: 'Hourly' })).toHaveAttribute('aria-checked', 'true')
  await scope(page)
  await expect(page.getByRole('button', { name: /^Interval:/ })).toHaveCount(0)
  await page.keyboard.press('Escape')

  const legend = chart.locator('ul').first()
  await expect(legend.getByText('Estimate', { exact: true })).toBeVisible()
  await expect(legend.getByText(/Typical \w+day$/)).toBeVisible()
  await expect(chart.getByText(/Now \d{1,2}:\d\d( [AP]M)?$/)).toBeVisible()
  await expect(chart.getByText(/^Exact counts up to now\. The estimate comes from the last \d+ \w+days\.$/)).toBeVisible()
  const table = chart.locator('table')
  await expect(table.locator('th', { hasText: /Estimate$/ })).toHaveCount(1)
  await expect(table.locator('th', { hasText: /Typical \w+day$/ })).toHaveCount(1)
  await expect(table.locator('td', { hasText: /^Estimate [\d,]+, between [\d,]+ and [\d,]+$/ }).first()).toBeAttached()
  await expect(table.locator('caption')).toContainText(/The estimate comes from the last \d+ \w+days\./)

  // The last hour of today is an estimate whatever the time; its tooltip says so and names the basis.
  const plot = chart.locator('.h-64').nth(1)
  const box = await plot.boundingBox()
  if (!box) throw new Error('the plot has no box')
  await page.mouse.move(box.x + box.width - 4, box.y + box.height / 2)
  const tooltip = chart.locator('.shadow-soft-lg')
  await expect(tooltip).toContainText(/Estimate [\d,]+, between [\d,]+ and [\d,]+/)
  await expect(tooltip).toContainText(/The estimate comes from the last \d+ \w+days\./)
  await expect(tooltip).toContainText(/Typical \w+day/)
  await page.mouse.move(0, 0)

  for (const scheme of ['dark', 'light'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 })
      await expectNoHorizontalOverflow(page)
      await panel.scrollIntoViewIfNeeded()
      await shot(page, `chart-overview-${width}-${scheme}`)
      if (shots) await panel.screenshot({ path: join(shots, `chart-panel-${width}-${scheme}.png`) })
    }
  }
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.setViewportSize({ width: 1440, height: 900 })

  // Daily is one state and one request; Back returns to hourly.
  const before = explores.length
  await intervals.getByRole('radio', { name: 'Daily' }).click()
  await expect.poll(() => (urlState(page) as { granularity: string } | null)?.granularity).toBe('day')
  await expect(intervals.getByRole('radio', { name: 'Daily' })).toHaveAttribute('aria-checked', 'true')
  expect(explores.length).toBe(before + 1)
  const daily = urlState(page)
  await page.goBack()
  await expect(intervals.getByRole('radio', { name: 'Hourly' })).toHaveAttribute('aria-checked', 'true')

  // A past range has no estimate and no Now marker.
  const yesterday = new Date(Date.now() - 86_400_000).toISOString().slice(0, 10)
  const past = { ...(daily as object), granularity: 'hour', range: { kind: 'days', start: yesterday, end: yesterday } }
  const encoded = Buffer.from(JSON.stringify(past)).toString('base64url')
  await page.goto(`/sites/${site}?s=${encoded}`)
  await expect(chart.locator('table')).toBeAttached()
  await expect(chart.getByText(/Now \d{1,2}:\d\d( [AP]M)?$/)).toHaveCount(0)
  await expect(legend.getByText('Estimate', { exact: true })).toHaveCount(0)
  await expect(legend.getByText(/Typical \w+day$/)).toBeVisible()
  await page.context().close()
})

test('breakdowns switch between Top and Movers within the one snapshot', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const site = await roadhouse(page)
  const explores: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('analytics.explore')) explores.push(request.url())
  })
  await page.goto(`/sites/${site}`)
  await details(page)
  const pages = page.getByRole('table', { name: 'Pages' })
  await expect(pages).toContainText('/de/events')
  await expect(pages.getByRole('columnheader', { name: 'Share' })).toBeVisible()
  const panel = page.locator('section').filter({ has: pages })
  await expect(panel).toContainText(/· [\d,]+ pages/)
  await expect(page.getByRole('table', { name: 'Referrers' }).getByRole('columnheader', { name: 'Share' })).toBeVisible()
  // Browsers, systems and devices share one panel behind a switch.
  const technology = page.getByRole('radiogroup', { name: 'Technology' })
  for (const [view, name] of [['Browsers', 'Browsers'], ['Systems', 'Operating systems'], ['Devices', 'Devices']]) {
    await technology.getByRole('radio', { name: view }).click()
    await expect(page.getByRole('table', { name }).getByRole('columnheader', { name: 'Share' })).toBeVisible()
  }
  const before = explores.length
  const listed = await pages.getByRole('row').count()

  // Movers re-sort the same page: no new request, a change column, quiet rows still listed.
  await panel.getByRole('button', { name: 'Movers', exact: true }).click()
  await expect(panel.getByRole('button', { name: 'Movers', exact: true })).toHaveAttribute('aria-pressed', 'true')
  await expect(pages.getByRole('columnheader', { name: 'Change' })).toBeVisible()
  await expect(pages.getByRole('row')).toHaveCount(listed)
  const changes = await pages.locator('tbody tr td:last-child').allTextContents()
  const sizes = changes.map((text) => Math.abs(Number(text.replace(/[^\d]/g, '')) || 0))
  expect(sizes).toEqual(sizes.toSorted((a, b) => b - a))
  expect(explores.length).toBe(before)
  for (const scheme of ['dark', 'light'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of [1440, 390]) {
      // The canvas scrolls inside the shell, so a tall viewport shows every table at once.
      await page.setViewportSize({ width, height: width > 400 ? 2200 : 4400 })
      await expect(() => expectNoHorizontalOverflow(page)).toPass()
      await shot(page, `tables-movers-${width}-${scheme}`)
    }
  }
  await page.emulateMedia({ colorScheme: 'dark' })
  await page.setViewportSize({ width: 1440, height: 900 })

  // Row actions stay in the menu: Only keeps the value as a filter.
  const first = pages.getByRole('row').nth(1)
  const value = (await first.getByRole('rowheader').innerText()).trim()
  await first.hover()
  await first.getByRole('button', { name: `Actions for ${value}` }).click()
  await page.getByRole('menuitem', { name: `Only ${value}` }).click()
  await expect.poll(() => JSON.stringify(urlState(page))).toContain(value)
  await page.goBack()

  // Without a comparison Movers is disabled and says why.
  await scope(page)
  await page.getByRole('button', { name: /^Comparison:/ }).click()
  await page.getByRole('menuitemradio', { name: 'No comparison' }).click()
  await expect.poll(() => (urlState(page) as { compare: string } | null)?.compare).toBe('none')
  const movers = panel.getByRole('button', { name: 'Movers', exact: true })
  await expect(movers).toBeDisabled()
  await expect(movers).toHaveAccessibleDescription('Movers need a comparison. Pick one in the scope chips.')
  await expect(pages.getByRole('columnheader', { name: 'Share' })).toBeVisible()
  await page.setViewportSize({ width: 1440, height: 2200 })
  await shot(page, 'tables-top-no-comparison-1440-dark')
  await page.context().close()
})

test('goal pace: one goals request beside the snapshot, rows in words with labelled estimates', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const site = await roadhouse(page)
  const name = `Event visit ${Date.now()}`
  const headers = { 'x-holocron-request': '1', Origin: new URL(page.url()).origin }
  const saved = await page.request.post('/api/commands/goals.save', {
    headers,
    data: {
      idempotency_key: crypto.randomUUID(),
      site,
      name,
      definition: {
        predicate: {
          kind: 'group',
          op: 'and',
          items: [{ kind: 'cond', id: 'c1', field: 'path', cmp: 'starts_with', value: { t: 'string', v: '/de/events' } }],
        },
        scope: 'session',
        counting: 'once',
      },
    },
  })
  expect(saved.ok(), await saved.text()).toBe(true)
  const { goal } = (await saved.json()) as { goal: { id: string } }

  const bodies: string[] = []
  await page.setViewportSize({ width: 1440, height: 2200 })
  await page.goto(`/sites/${site}`)
  // The today view has its own request; count from the moment the details view opens.
  await expect(page.getByText(/Pageviews today, until \d/)).toBeVisible()
  page.on('request', (request) => {
    if (request.url().includes('analytics.explore')) bodies.push(decodeURIComponent(request.url()))
  })
  await details(page)
  const goals = page.getByRole('table', { name: 'Goals' })
  const row = goals.getByRole('row', { name: new RegExp(name) })
  await expect(row).toContainText(/\d+ so far, \d+ usual by now|Not enough history yet/)
  await expect(row.getByRole('cell').last()).toHaveText(/behind pace|^On pace|ahead of pace|Not enough history yet/)
  const estimate = row.getByRole('button', { name: /^Estimate: / })
  if ((await estimate.count()) > 0) await expect(estimate).toHaveAccessibleName(/Between [\d,]+ and [\d,]+\. Estimate from /)

  // Two requests for the site: the snapshot and one goals report with a typical weekday.
  const distinct = [...new Set(bodies.filter((body) => body.includes(`"site":"${site}"`)))]
  const reports = distinct.filter((body) => body.includes('"kind":"goals"'))
  expect(reports).toHaveLength(1)
  expect(reports[0]).toContain('"compare":"typical_weekday"')
  expect(distinct.length - reports.length).toBe(1)
  await expect(page.getByRole('table', { name: 'Pages' })).toContainText('/de')
  await shot(page, 'tables-goals-1440-dark')

  await row.getByRole('link', { name: `Open ${name} in the Goals report` }).click()
  await page.waitForURL(/\/goals\?s=/)
  expect((urlState(page) as { compare: string }).compare).toBe('typical_weekday')
  // The overview lists at most eight goals; archiving keeps later runs inside that limit.
  const archived = await page.request.post('/api/commands/goals.archive', {
    headers,
    data: { idempotency_key: crypto.randomUUID(), site, goal: goal.id },
  })
  expect(archived.ok(), await archived.text()).toBe(true)
  await page.context().close()
})

test('acceptance workflow: filter, split, drill, compare, save, pin and reopen the same state', async ({ browser }) => {
  test.setTimeout(120_000)
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/explore`)
  await expect(page.getByRole('heading', { level: 1, name: 'Explore' })).toBeVisible()

  await scope(page)
  await page.getByRole('button', { name: /^Date range:/ }).click()
  await page.getByRole('menuitemradio', { name: 'Last 30 days' }).click()
  await addFilter(page, 'Device', 'is', 'mobile')
  await addFilter(page, 'Event', 'is', 'signup')
  await addFilter(page, 'plan (event property)', 'is', 'pro')

  await page.getByRole('button', { name: /^Measures/ }).click()
  await page.getByRole('menuitemcheckbox', { name: 'Events' }).click()
  await page.getByRole('menuitemcheckbox', { name: 'Views' }).click()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Remove Page' }).click()
  for (const field of ['Country', 'Referrer']) {
    await page.getByRole('button', { name: 'Add a dimension' }).click()
    await page.getByRole('menuitem', { name: field, exact: true }).click()
  }
  await scope(page)
  await expect(page.getByRole('button', { name: 'Comparison: vs previous period' })).toBeVisible()

  const table = page.getByRole('region', { name: 'Results' })
  await expect(table.getByRole('row').nth(1)).toContainText('Austria')
  const before = urlState(page) as { filter: { items: unknown[] }; dimensions: unknown[] }
  expect(before.dimensions).toEqual(['country', 'referrer_domain'])
  await shot(page, 'acceptance-breakdown-light', true)

  // Drill into Austria: one more condition, nothing else changes.
  await table.getByRole('button', { name: /^Actions for Austria/ }).first().click()
  await page.getByRole('menuitem', { name: 'Only Austria' }).click()
  await expect.poll(() => (urlState(page) as typeof before).filter.items.length).toBe(4)
  const drilled = urlState(page)
  await scope(page)
  await expect(page.getByRole('button', { name: 'Edit the filter on Country' })).toBeVisible()
  await page.keyboard.press('Escape')
  await shot(page, 'acceptance-drilled-light', true)

  // Back undoes exactly the drill-down, Forward redoes it.
  await page.goBack()
  await expect.poll(() => urlState(page)).toEqual(before)
  await page.goForward()
  await expect.poll(() => urlState(page)).toEqual(drilled)

  // Save the view; the bookmark marks it active.
  const viewName = `Mobile pro signups ${Date.now()}`
  await page.getByRole('button', { name: /^Saved view/ }).click()
  await page.getByRole('menuitem', { name: 'Save this view' }).click()
  await page.getByRole('textbox', { name: 'Name this view' }).fill(viewName)
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: /^Saved view: Mobile pro signups/ })).toBeVisible()

  // Export carries the same state.
  const download = page.waitForEvent('download')
  await page.getByRole('button', { name: 'Export' }).click()
  await page.getByRole('menuitem', { name: 'Download JSON with state' }).click()
  const file = JSON.parse(readFileSync(await (await download).path(), 'utf8')) as { state: unknown }
  expect(file.state).toEqual(drilled)

  // Pin to a new board, then reopen from the board.
  await page.getByRole('button', { name: 'Pin to board' }).click()
  const dialog = page.getByRole('dialog')
  const boardName = `Growth ${Date.now()}`
  await dialog.getByLabel('Board', { exact: true }).selectOption('')
  await dialog.getByLabel('New board name').fill(boardName)
  await dialog.getByLabel('Card title').fill('Mobile pro signups by country')
  await dialog.getByRole('button', { name: 'Pin' }).click()
  await expect(page.getByText(`Pinned to ${boardName}.`)).toBeVisible()

  await page.getByRole('navigation', { name: 'Analytics navigation' }).getByRole('link', { name: 'Boards' }).click()
  await page.getByRole('link', { name: new RegExp(boardName) }).click()
  await expect(page.getByRole('heading', { level: 2, name: 'Mobile pro signups by country' })).toBeVisible()
  await expect(page.getByText('Pinned from a saved view')).toHaveCount(0)
  await shot(page, 'board-light', true)
  await page.getByRole('button', { name: 'Options for Mobile pro signups by country' }).click()
  await page.getByRole('menuitem', { name: 'Open in Explore' }).click()
  await page.waitForURL(/\/explore\?/)
  expect(urlState(page)).toEqual(drilled)

  // Reopen the saved view on a fresh Explore page.
  await page.goto(`/sites/${site}/explore`)
  await page.getByRole('button', { name: /^Saved view/ }).click()
  await page.getByRole('menuitem', { name: viewName }).click()
  await expect.poll(() => urlState(page)).toEqual(drilled)
  await page.context().close()
})

test('clearing filters keeps dates, measures and dimensions; Reset is separate', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/explore`)
  await scope(page)
  await page.getByRole('button', { name: /^Date range:/ }).click()
  await page.getByRole('menuitemradio', { name: 'Last 90 days' }).click()
  await addFilter(page, 'Device', 'is', 'mobile')
  await addFilter(page, 'Device', 'is', 'tablet')
  // Group the two device conditions: (mobile OR tablet).
  await (await filterEditor(page)).getByRole('button', { name: /^and\. Group with the previous filter/ }).click()
  await expect(page.getByRole('group', { name: 'Group of 2 conditions joined with or' })).toBeVisible()
  await shot(page, 'filter-group-dark')
  const grouped = urlState(page) as Record<string, unknown>

  await (await filterEditor(page)).getByRole('button', { name: 'Clear filters' }).click()
  const cleared = urlState(page) as Record<string, unknown>
  expect({ ...cleared, filter: grouped.filter }).toEqual({ ...grouped, page: cleared.page })
  expect(cleared.filter).toBeNull()

  // A link this dashboard cannot read is refused with a way out, not computed.
  await page.goto(`/sites/${site}/explore?s=bm90LWEtc3RhdGU`)
  await expect(page.getByText('This link holds a view this dashboard cannot read.')).toBeVisible()
  await shot(page, 'unreadable-link-dark')
  await page.getByRole('button', { name: 'Reset view' }).click()
  await expect(page.getByRole('region', { name: 'Results' })).toBeVisible()

  // A refused state names the refused condition.
  const unknown = {
    ...(grouped as object),
    filter: { kind: 'group', op: 'and', items: [{ kind: 'cond', id: 'f1', field: { scope: 'event', key: 'nope' }, cmp: 'eq', value: { t: 'string', v: 'x' } }] },
  }
  await page.goto(`/sites/${site}/explore?s=${Buffer.from(JSON.stringify(unknown)).toString('base64url')}`)
  await expect(page.getByText('This view cannot run')).toBeVisible()
  await shot(page, 'refused-condition-dark')
  await page.context().close()
})

test('phone: filters collapse, tables scroll in their region, controls stay reachable', async ({ browser }) => {
  for (const scheme of ['light', 'dark'] as const) {
    const page = await signedIn(browser, admin, { viewport: { width: 390, height: 844 }, colorScheme: scheme })
    const site = await roadhouse(page)
    await page.goto(`/sites/${site}/explore`)
    await addFilter(page, 'Device', 'is', 'mobile')
    await expect((await filterEditor(page)).getByRole('button', { name: '1 filters' })).toBeVisible()
    await expectNoHorizontalOverflow(page)
    await shot(page, `explore-390-${scheme}`, true)
    await page.context().close()
  }
})

test('realtime follows new data live and shows its connection state', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/realtime`)
  await expect(page.getByRole('status').filter({ hasText: 'Live' })).toBeVisible()
  await page.getByRole('radiogroup', { name: 'View' }).getByRole('radio', { name: 'Details' }).click()
  const active = page.getByText('Active now').locator('..').locator('p').nth(1)
  const before = Number((await active.textContent())?.replace(/\D/g, ''))

  const sent = await page.request.post('/api/send', {
    headers: {
      'content-type': 'application/json',
      'user-agent': 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
    },
    data: { type: 'event', payload: { website: site, hostname: 'roadhouse.test', url: '/de/live-check', screen: '390x844', language: 'de-AT' } },
  })
  expect(sent.ok()).toBe(true)
  await expect(page.getByRole('cell', { name: /\/de\/live-check/ }).first()).toBeVisible({ timeout: 15_000 })
  await expect.poll(async () => Number((await active.textContent())?.replace(/\D/g, ''))).toBeGreaterThanOrEqual(before)
  await shot(page, 'realtime-light', true)
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot(page, 'realtime-dark', true)
  await page.context().close()
})

test('a revoked site clears its data and the server refuses its reads', async ({ browser }) => {
  const database = process.env.HOLOCRON_E2E_DB
  test.skip(!database || viewer.password === '', 'set HOLOCRON_E2E_DB and HOLOCRON_E2E_VIEWER_PASSWORD')
  const page = await signedIn(browser, viewer, { colorScheme: 'light' })
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/explore`)
  await expect(page.getByRole('region', { name: 'Results' })).toBeVisible()

  const sql = (statement: string) => execFileSync('sqlite3', [database ?? '', statement]).toString()
  const row = sql(
    `SELECT m.workspace_id || '|' || m.user_id FROM memberships m JOIN users u ON u.id = m.user_id JOIN sites s ON s.workspace_id = m.workspace_id WHERE u.email = 'viewer@example.test' AND s.id = '${site}'`
  ).trim()
  const [workspace, user] = row.split('|')
  sql(`DELETE FROM memberships WHERE workspace_id = '${workspace}' AND user_id = '${user}'`)
  try {
    await page.getByRole('button', { name: 'Refresh', exact: true }).click()
    await expect(page.getByText('Website not available')).toBeVisible()
    await expect(page.getByRole('region', { name: 'Results' })).toHaveCount(0)
    await shot(page, 'revoked-scope-light')
    const state = {
      v: 1, site, measures: ['pageviews'], dimensions: [], filter: null, range: { kind: 'relative', unit: 'day', count: 7 },
      timezone: 'Europe/Vienna', granularity: 'day', compare: 'none', metric_version: 1, sort: [], page: { size: 50, index: 0 },
      visualization: 'table',
    }
    const exported = await page.request.get(`/api/queries/analytics.export?format=csv&state=${encodeURIComponent(JSON.stringify(state))}`, {
      headers: { 'x-holocron-request': '1' },
    })
    expect(exported.status()).toBe(404)
  } finally {
    sql(`INSERT INTO memberships (workspace_id, user_id, role) VALUES ('${workspace}', '${user}', 'viewer')`)
  }
  await page.context().close()
})

test('the command palette jumps to a saved view with the keyboard alone', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  const site = await roadhouse(page)

  const viewName = `Palette jump ${Date.now()}`
  await page.goto(`/sites/${site}/explore`)
  await page.getByRole('button', { name: /^Saved view/ }).click()
  await page.getByRole('menuitem', { name: 'Save this view' }).click()
  await page.getByRole('textbox', { name: 'Name this view' }).fill(viewName)
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: /^Saved view: Palette jump/ })).toBeVisible()

  // Escape gives focus back to where it was.
  await page.goto(`/sites/${site}`)
  const menu = page.getByRole('button', { name: 'Account menu' })
  await menu.focus()
  await page.keyboard.press('Control+k')
  const input = page.getByPlaceholder('Search pages, websites and actions')
  await expect(input).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await page.keyboard.press('ArrowDown')
  await expect(page.locator('[role=option][data-highlighted]')).toHaveCount(1)
  await page.keyboard.press('Escape')
  await expect(menu).toBeFocused()

  // Typing the name and pressing Enter opens the view with its exact state.
  await page.keyboard.press('Control+k')
  await page.keyboard.type(viewName)
  await expect(page.getByRole('option', { name: viewName })).toHaveAttribute('data-highlighted', '')
  await page.keyboard.press('Enter')
  await page.waitForURL(/\/explore\?/)
  await expect(page.getByRole('button', { name: `Saved view: ${viewName}` })).toBeVisible()
  await page.context().close()
})
