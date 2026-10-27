import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { admin, sessionFile } from './session'

/**
 * The events report (CAP-TRAFFIC-06 and 07) against a server seeded with `seed_demo` and
 * `seed_traffic`: event names with counts and a series, the properties of one event, numeric
 * statistics, a pivot, keyboard and Back, at desktop and phone widths in both modes. Set
 * HOLOCRON_E2E_PASSWORD; HOLOCRON_E2E_SHOTS writes screenshots.
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

interface EventsState {
  dimensions: unknown[]
  measures: unknown[]
  filter: { items: Array<{ field: unknown; cmp: string; value?: { v: unknown } }> } | null
}

function urlState(page: Page): EventsState | null {
  const s = new URL(page.url()).searchParams.get('s')
  return s ? (JSON.parse(Buffer.from(s, 'base64url').toString('utf8')) as EventsState) : null
}

/** Checkout events with a decimal, an integer, an array and a date, through the tracker. */
async function sendCheckouts(page: Page, site: string) {
  const amounts = [49.9, 12.5, 49.9, 120]
  for (const [index, amount] of amounts.entries()) {
    const sent = await page.request.post('/api/send', {
      headers: {
        'content-type': 'application/json',
        'user-agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
      },
      data: {
        type: 'event',
        payload: {
          website: site,
          hostname: 'roadhouse.test',
          url: '/de/tickets',
          name: 'checkout',
          screen: '1440x900',
          language: 'de-AT',
          data: { amount, items: index + 1, seats: ['A1', `B${index}`], show: '2026-12-31T20:00:00.000Z' },
        },
      },
    })
    expect(sent.ok()).toBe(true)
  }
}

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
  expect(overflow).toBe(0)
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('events: names with counts and a series, properties of one event, numbers, pivot and Back', async ({ browser }) => {
  test.setTimeout(120_000)
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await sendCheckouts(page, site)

  await page.goto(`/sites/${site}`)
  await page.getByRole('link', { name: 'Events' }).first().click()
  await page.waitForURL(`**/sites/${site}/events`)
  await expect(page.getByRole('heading', { level: 1, name: 'Events' })).toBeVisible()
  // The board opens first: a row opens the event's properties in Details, and Back returns to the board.
  const signupRow = page.getByRole('button', { name: 'Properties of signup' })
  await signupRow.click()
  await expect(page.getByRole('radio', { name: 'Properties' })).toHaveAttribute('aria-checked', 'true')
  await page.goBack()
  await expect(signupRow).toBeVisible()
  await page.getByRole('radio', { name: 'Details' }).click()
  const results = page.getByRole('region', { name: 'Results' })
  await expect(results.getByRole('row', { name: /signup/ })).toBeVisible()
  // The chart has a text summary and a table with the same values.
  await expect(page.locator('figure figcaption')).toContainText('Events')
  await expect(page.locator('figure .sr-only table tbody tr').first()).toBeAttached()
  // The checkouts arrive through collection and correction; a reload reads them once they do.
  await expect.poll(async () => {
    await page.reload()
    await expect(results.getByRole('row', { name: /signup/ })).toBeVisible()
    return results.getByRole('row', { name: /checkout/ }).count()
  }, { timeout: 30_000 }).toBe(1)
  await shot(page, 'events-1440-light')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot(page, 'events-1440-dark')
  await page.emulateMedia({ colorScheme: 'light' })

  // A row action opens the event's properties; the selection is one condition in the URL state.
  await results.getByRole('button', { name: 'Actions for signup' }).click()
  await page.getByRole('menuitem', { name: 'Properties of signup' }).click()
  await expect(page.getByRole('radio', { name: 'Properties' })).toHaveAttribute('aria-checked', 'true')
  const selections = (urlState(page)?.filter?.items ?? []).filter((c) => c.field === 'event_name' && c.cmp === 'eq')
  expect(selections.map((c) => c.value?.v)).toEqual(['signup'])

  await page.getByRole('combobox', { name: 'Property' }).click()
  await page.getByRole('option', { name: 'plan' }).click()
  await expect(page.getByRole('heading', { name: 'Values of plan' })).toBeVisible()
  await expect(results.getByRole('button', { name: 'Actions for pro' })).toBeVisible()
  await expect(results.getByRole('button', { name: 'Actions for free' })).toBeVisible()
  await shot(page, 'events-properties-1440-light')

  // Numbers: statistics, sum and mean over time, values pivoted by a second property.
  await page.getByRole('combobox', { name: 'Event' }).click()
  await page.getByRole('option', { name: 'checkout' }).click()
  await page.getByRole('combobox', { name: 'Property' }).click()
  await page.getByRole('option', { name: 'amount' }).click()
  const stats = page.getByRole('region', { name: 'Statistics of amount' })
  await expect(stats.getByText('Sum of amount').first()).toBeVisible()
  await expect(stats.getByText('Maximum of amount')).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Mean of amount over time' })).toBeVisible()
  await expect(results.getByRole('button', { name: 'Actions for 49.9000' })).toBeVisible()
  const numeric = urlState(page)
  await page.getByRole('combobox', { name: 'Pivot by' }).click()
  await page.getByRole('option', { name: 'items' }).click()
  await expect(results.getByRole('columnheader', { name: /items/ })).toBeVisible()
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot(page, 'events-numeric-1440-dark')
  await page.emulateMedia({ colorScheme: 'light' })

  // Back undoes exactly the pivot.
  await page.goBack()
  await expect.poll(() => urlState(page)).toEqual(numeric)
  await expect(results.getByRole('columnheader', { name: /items/ })).toHaveCount(0)

  // The view switch is a radio group: arrow keys move between the list and the properties.
  await page.getByRole('radio', { name: 'Properties' }).focus()
  await page.keyboard.press('ArrowLeft')
  await expect(page.getByRole('radio', { name: 'Events' })).toHaveAttribute('aria-checked', 'true')
  await expect(results.getByRole('row', { name: /signup/ })).toBeVisible()
  expect(urlState(page)?.dimensions).toEqual(['event_name'])
  await page.context().close()
})

test('events at phone width in both modes keep the page inside the screen', async ({ browser }) => {
  const page = await signedIn(browser, { viewport: { width: 390, height: 844 }, hasTouch: true })
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/events`)
  await expect(page.getByRole('button', { name: 'Properties of signup' })).toBeVisible()
  await expectNoHorizontalOverflow(page)
  await shot(page, 'events-board-390-light')
  await page.getByRole('radio', { name: 'Details' }).click()
  await expect(page.getByRole('region', { name: 'Results' }).getByRole('row', { name: /signup/ })).toBeVisible()
  await expectNoHorizontalOverflow(page)
  await shot(page, 'events-390-light')
  await page.emulateMedia({ colorScheme: 'dark' })
  await shot(page, 'events-390-dark')
  await page.context().close()
})
