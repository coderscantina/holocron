import { expect, test, type Browser, type BrowserContextOptions } from '@playwright/test'
import { admin, sessionFile } from './session'

/**
 * A funnel split by a field and the time between steps, against a server seeded with
 * `seed_demo` and `seed_traffic`: roadhouse.test has pageviews from several countries and
 * `signup` events in the sessions' last pages, so the default funnel (any page view, then any
 * event) has visitors in more than one country and some reach step 2. Set HOLOCRON_E2E_PASSWORD.
 */

async function signedIn(browser: Browser, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', ...options, storageState: sessionFile(admin) })
  return context.newPage()
}

async function roadhouse(page: import('@playwright/test').Page): Promise<string> {
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> }
  const site = workspaces.flatMap((w) => w.sites).find((s) => s.name === 'roadhouse.test')
  if (!site) throw new Error('seed_demo did not run')
  return site.id
}

interface FunnelUrlState {
  report: { v: number; group_by?: unknown }
  filter: { items: Array<{ field: unknown; cmp: string }> } | null
}

function urlState(url: string): FunnelUrlState {
  const s = new URL(url).searchParams.get('s')
  if (!s) throw new Error('no state in the URL')
  return JSON.parse(Buffer.from(s, 'base64url').toString('utf8')) as FunnelUrlState
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('funnel split: table by country, pick a group, a median between steps, reload keeps the split', async ({ browser }) => {
  test.setTimeout(90_000)
  const page = await signedIn(browser)
  const site = await roadhouse(page)
  await page.goto(`/sites/${site}/funnels`)
  await expect(page.getByRole('heading', { level: 1, name: 'Funnels' })).toBeVisible()

  const conversion = page.getByText(/reached the last step/).first()
  await expect(conversion).toBeVisible()
  const overall = await conversion.innerText()

  await page.getByRole('button', { name: 'Split by' }).click()
  await page.getByRole('menuitem', { name: 'Country' }).click()
  const table = page.getByRole('region', { name: /^Funnel by Country/ })
  await expect(table).toBeVisible()
  await expect(table.getByRole('button', { name: 'All traffic' })).toBeVisible()
  expect(urlState(page.url()).report).toMatchObject({ v: 2, group_by: 'country' })

  // The median from step 1 to step 2 sits between the columns and opens the spread.
  const median = page.getByRole('button', { name: /^Median .+ from step 1 to step 2/ })
  await expect(median).toBeVisible()
  await median.focus()
  await expect(page.getByText(/^Step 1 to 2 · /)).toBeVisible()
  await expect(page.getByText('Under 1 h')).toBeVisible()
  await page.keyboard.press('Escape')

  // A row reads that group's own funnel; the split stays in the URL.
  await table.getByRole('button', { name: /^Show the funnel for / }).first().click()
  await expect(page.getByRole('button', { name: 'All traffic' }).first()).toBeVisible()
  await expect(conversion).not.toHaveText(overall)
  expect(urlState(page.url()).report).toMatchObject({ v: 2, group_by: 'country' })

  // All traffic returns to the overall result.
  await table.getByRole('button', { name: 'All traffic' }).click()
  await expect(conversion).toHaveText(overall)

  await page.reload()
  await expect(page.getByRole('button', { name: 'Remove the split by Country' })).toBeVisible()
  await expect(page.getByRole('region', { name: /^Funnel by Country/ })).toBeVisible()
  await page.context().close()
})
