import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { admin, sessionFile } from './session'

/**
 * Milestone 07 journeys against a server seeded with `seed_demo` and `seed_traffic`: goals with
 * versions, a funnel built step by step and carried through save, export, pin and the board,
 * drill-downs into sessions, journeys, campaigns and attribution, and every report screen at
 * desktop and phone widths in both modes. Set HOLOCRON_E2E_PASSWORD; HOLOCRON_E2E_SHOTS
 * writes screenshots.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

async function shot(page: Page, name: string) {
  if (!shots) return
  await expect(page.getByText('Showing the previous result while this one loads.')).toHaveCount(0)
  await page.evaluate(() =>
    Promise.all(document.getAnimations().map((animation) => animation.finished.catch(() => undefined)))
  )
  await page.screenshot({ path: join(shots, `${name}.png`), fullPage: true })
}

async function signedIn(browser: Browser, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', ...options, storageState: sessionFile(admin) })
  return context.newPage()
}

async function roadhouse(page: Page): Promise<string> {
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> }
  const site = workspaces.flatMap((w) => w.sites).find((s) => s.name === 'roadhouse.test')
  if (!site) throw new Error('seed_demo did not run')
  return site.id
}

function urlState(page: Page): Record<string, unknown> | null {
  const s = new URL(page.url()).searchParams.get('s')
  return s ? (JSON.parse(Buffer.from(s, 'base64url').toString('utf8')) as Record<string, unknown>) : null
}

interface FunnelState {
  report: { kind: string; steps: Array<{ items: Array<{ id: string; field: unknown; value?: { v: unknown } }> }>; window_seconds: number }
}

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBe(0)
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('goals: create, see conversion, edit into a new version, upgrade a saved report', async ({ browser }) => {
  test.setTimeout(90_000)
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/goals`)
  await expect(page.getByRole('heading', { level: 1, name: 'Goals' })).toBeVisible()

  const name = `Signup ${Date.now()}`
  await page.getByRole('button', { name: 'New goal' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Name').fill(name)
  const input = dialog.getByRole('combobox', { name: 'Matching events' })
  await input.fill('Event')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await dialog.getByRole('combobox').fill('signup')
  await page.keyboard.press('Enter')
  await expect(dialog.getByRole('button', { name: 'Edit the filter on Event' })).toBeVisible()
  await dialog.getByRole('button', { name: 'Create goal' }).click()
  await expect(dialog).toHaveCount(0)

  const table = page.getByRole('region', { name: 'Goal conversion' })
  const row = table.getByRole('row', { name: new RegExp(name) })
  await expect(row).toContainText('v1')
  await expect(row.getByRole('cell').first()).toHaveText(/%$/)
  const first = urlState(page)

  // Editing saves version 2; the report on screen moves to it, a saved link keeps version 1.
  await row.getByRole('button', { name: `Actions for ${name}` }).click()
  await page.getByRole('menuitem', { name: 'Edit goal (new version)' }).click()
  await page.getByRole('dialog').getByRole('radio', { name: 'Every event' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Save new version' }).click()
  await expect(row).toContainText('v2')
  await expect(row).toContainText('Every event')

  await page.goto(`/sites/${site}/goals?s=${Buffer.from(JSON.stringify(first)).toString('base64url')}`)
  await expect(row).toContainText('v1')
  await row.getByRole('button', { name: `Actions for ${name}` }).click()
  await page.getByRole('menuitem', { name: 'Use version 2' }).click()
  await expect(row).toContainText('v2')
  await page.context().close()
})

test('funnel: build steps by keyboard, save, export, pin, reopen and drill into sessions', async ({ browser }) => {
  test.setTimeout(120_000)
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/funnels`)
  await expect(page.getByRole('heading', { level: 1, name: 'Funnels' })).toBeVisible()
  // The step editor and the step table live in the details view; the ribbon opens first.
  await page.getByRole('radio', { name: 'Details' }).click()

  // Two new steps through the quick form, then the defaults go.
  const add = page.getByRole('textbox', { name: 'Page path, like /pricing' })
  await add.fill('/de/events')
  await add.press('Enter')
  await page.getByRole('radio', { name: 'Page' }).focus()
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('radio', { name: 'Event' })).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('textbox', { name: 'Event name, like signup' }).fill('signup')
  await page.keyboard.press('Enter')
  await expect.poll(() => (urlState(page) as unknown as FunnelState).report.steps.length).toBe(4)
  await page.getByRole('button', { name: 'Remove step 1' }).click()
  await page.getByRole('button', { name: 'Remove step 1' }).click()
  await page.getByRole('radio', { name: '1 day' }).click()

  const state = urlState(page) as unknown as FunnelState
  expect(state.report.steps.map((step) => step.items[0]?.value?.v)).toEqual(['/de/events', 'signup'])
  expect(state.report.window_seconds).toBe(86_400)

  const table = page.getByRole('region', { name: 'Funnel steps and how many reached each' })
  await expect(table.getByRole('row', { name: /Step 2/ })).toBeVisible()
  await expect(page.getByText(/reached the last step/)).toBeVisible()
  await expect(page.getByText(/Daily scope follows each visitor/)).toBeVisible()

  // Moving a step is a button press, and Back undoes it.
  await page.getByRole('button', { name: 'Move step 2 up' }).click()
  await expect.poll(() => (urlState(page) as unknown as FunnelState).report.steps[0]?.items[0]?.value?.v).toBe('signup')
  await page.goBack()
  await expect.poll(() => urlState(page)).toEqual(state)

  const viewName = `Events to signup ${Date.now()}`
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
    result: { report: { steps: Array<{ reached: number }> } }
  }
  expect(file.state).toEqual(state)
  const reached = file.result.report.steps[0]?.reached ?? 0
  await expect(table.getByRole('row', { name: /Step 1/ })).toContainText(new Intl.NumberFormat('en').format(reached))

  await page.getByRole('button', { name: 'Pin to board' }).click()
  const dialog = page.getByRole('dialog')
  const boardName = `Funnels ${Date.now()}`
  await dialog.getByLabel('Board', { exact: true }).selectOption('')
  await dialog.getByLabel('New board name').fill(boardName)
  await dialog.getByLabel('Card title').fill('Events to signup')
  await dialog.getByRole('button', { name: 'Pin' }).click()
  await expect(page.getByText(`Pinned to ${boardName}.`)).toBeVisible()

  await page.getByRole('navigation', { name: 'Analytics navigation' }).getByRole('link', { name: 'Boards' }).click()
  await page.getByRole('link', { name: new RegExp(boardName) }).click()
  await expect(page.getByText(/reached the last step/)).toBeVisible()
  await shot(page, 'board-funnel-light')
  await page.getByRole('button', { name: 'Options for Events to signup' }).click()
  await page.getByRole('menuitem', { name: /^Open/ }).click()
  await page.waitForURL(/\/funnels\?s=/)
  expect(urlState(page)).toEqual(state)

  // Step 2's sessions: the session list with the step's conditions.
  await page.getByRole('button', { name: 'Actions for Step 2' }).click()
  await page.getByRole('menuitem', { name: 'Show sessions' }).click()
  await page.waitForURL(/\/sessions\?s=/)
  const sessions = urlState(page) as { report: { kind: string }; filter: { items: Array<{ field: unknown }> } }
  expect(sessions.report.kind).toBe('session_list')
  expect(sessions.filter.items.map((item) => item.field)).toEqual(['event_name'])
  // The session list is the details view; the timeline opens first.
  await page.getByRole('radio', { name: 'Details' }).click()
  await expect(page.getByRole('region', { name: 'Sessions' }).getByRole('row').nth(1)).toBeVisible()
  await expect(page.getByText(/\d[\d,]* sessions$/)).toBeVisible()
  await page.context().close()
})

test('journeys, campaigns and attribution compute from the seeded traffic', async ({ browser }) => {
  test.setTimeout(90_000)
  const page = await signedIn(browser)
  const site = await roadhouse(page)

  await page.goto(`/sites/${site}/journeys`)
  await page.getByRole('radio', { name: 'Details' }).click()
  const journeys = page.getByRole('region', { name: 'Most common journeys' })
  await expect(journeys.getByRole('row', { name: /→/ }).first()).toBeVisible()
  await page.getByRole('radiogroup', { name: 'Steps', exact: true }).getByRole('radio', { name: '2' }).click()
  await expect.poll(() => (urlState(page) as { report: { length: number } }).report.length).toBe(2)
  await expect(page.getByText(/\d sessions, \d[\d,]* paths/)).toBeVisible()

  await page.goto(`/sites/${site}/campaigns`)
  await page.getByRole('radio', { name: 'Details' }).click()
  const campaigns = page.getByRole('region', { name: 'Results' })
  await expect(campaigns).toContainText('autumn-tour')
  await expect(campaigns).toContainText('birthday-party')
  await page.getByRole('radio', { name: 'UTM source' }).click()
  await expect(campaigns).toContainText('newsletter')

  await page.getByRole('link', { name: 'Attribute conversions' }).click()
  await page.waitForURL(/\/attribution\?s=/)
  await page.getByRole('radio', { name: 'Details' }).click()
  await page.getByRole('button', { name: 'Add conversion' }).click()
  await page.getByRole('menuitem', { name: /^Signup/ }).first().click()
  await page.getByRole('button', { name: 'Remove Revenue' }).click()
  const attributed = page.getByRole('region', { name: /^Attributed conversions for Signup/ })
  await expect(attributed).toContainText('newsletter')
  await expect(attributed).toContainText('Direct or no outside touch')
  await page.getByRole('combobox', { name: 'Credit by' }).click()
  await page.getByRole('option', { name: 'UTM campaign' }).click()
  await expect(attributed).toContainText('autumn-tour')
  await page.context().close()
})

test('every report screen at desktop and phone widths in both modes', async ({ browser }) => {
  test.setTimeout(180_000)
  for (const scheme of ['light', 'dark'] as const) {
    for (const width of [1440, 390]) {
      const page = await signedIn(browser, { colorScheme: scheme, viewport: { width, height: width === 390 ? 844 : 900 } })
      const site = await roadhouse(page)
      for (const route of ['funnels', 'goals', 'journeys', 'campaigns', 'attribution', 'sessions']) {
        await page.goto(`/sites/${site}/${route}`)
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible()
        await expect(page.locator('.animate-pulse')).toHaveCount(0)
        await expectNoHorizontalOverflow(page)
        await shot(page, `${route}-${width}-${scheme}`)
      }
      await page.context().close()
    }
  }
})
