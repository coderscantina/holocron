import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type Page } from '@playwright/test'
import { type Account, admin, viewer, sessionFile } from './session'

/**
 * The admin server status page against the packaged server and the `seed_demo` data. Set
 * HOLOCRON_E2E_PASSWORD (admin) and HOLOCRON_E2E_VIEWER_PASSWORD. With HOLOCRON_E2E_SHOTS set,
 * screenshots at 1440 and 390 px in both schemes go to that directory.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

test.describe.configure({ mode: 'serial', timeout: shots ? 120_000 : 30_000 })

async function signedIn(browser: Browser, account: Account): Promise<Page> {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', storageState: sessionFile(account) })
  const page = await context.newPage()
  await page.goto('/')
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))
  return page
}

test('an install admin reads the server state, forecast, budgets and maintenance', async ({ browser }) => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
  const page = await signedIn(browser, admin)
  await page.goto('/admin/users')
  await page.getByRole('navigation', { name: 'Administration sections' }).getByRole('link', { name: 'Server status' }).click()
  await expect(page).toHaveURL(/\/admin\/status$/)

  const state = page.getByRole('heading', { level: 2 }).first()
  await expect(state).toBeVisible()
  await expect(state).not.toHaveText(/_/)
  await expect(page.getByRole('heading', { name: 'Storage forecast' })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Budgets' })).toBeVisible()
  await expect(page.getByRole('progressbar', { name: 'Disk' })).toBeVisible()
  await expect(page.getByRole('table', { name: 'Maintenance' })).toBeVisible()

  if (shots) {
    await page.getByText('How it is computed').click()
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme })
      for (const width of [1440, 390]) {
        await page.setViewportSize({ width, height: 900 })
        await page.waitForTimeout(300)
        await page.screenshot({ path: join(shots, `admin-status-${width}-${scheme}.png`), fullPage: true })
      }
    }
  }
  await page.close()
})

test('people who are not install admins get the not-found page', async ({ browser }) => {
  expect(viewer.password, 'set HOLOCRON_E2E_VIEWER_PASSWORD').not.toBe('')
  const page = await signedIn(browser, viewer)
  const asked: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('/api/queries/operator.status')) asked.push(request.url())
  })
  await page.goto('/admin/status')
  await expect(page.getByText('Page not found')).toBeVisible()
  await expect(page.getByText('Server status')).toHaveCount(0)
  expect(asked).toEqual([])
  await page.close()
})
