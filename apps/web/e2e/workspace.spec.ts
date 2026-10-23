import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test } from '@playwright/test'

/**
 * The workspace sites page against the packaged server, `seed_demo` and `seed_traffic` data. Set
 * HOLOCRON_E2E_PASSWORD (admin). With HOLOCRON_E2E_SHOTS set, screenshots are written there.
 * Summaries appear after the background step's first round, so the journey waits for them.
 */
const email = process.env.HOLOCRON_E2E_EMAIL ?? 'mike@example.test'
const password = process.env.HOLOCRON_E2E_PASSWORD ?? ''
const shots = process.env.HOLOCRON_E2E_SHOTS

if (shots) mkdirSync(shots, { recursive: true })

test.beforeAll(() => {
  expect(password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('the workspace page shows how every site is doing from one summaries request', async ({ page }) => {
  test.setTimeout(240_000)
  const queries: string[] = []
  page.on('request', (request) => {
    const name = /\/queries\/([\w.]+)/.exec(request.url())?.[1]
    if (name) queries.push(name)
  })

  await page.goto('/login')
  await page.getByLabel(/^(Email|E-Mail)$/).fill(email)
  await page.getByLabel(/^(Password|Passwort)$/).fill(password)
  await page.getByRole('button', { name: /^(Sign in|Anmelden)$/ }).click()
  await page.waitForURL(/\/sites\//)

  await page.getByRole('button', { name: /\. (Switch workspace|Arbeitsbereich wechseln)$/ }).click()
  await page.getByRole('menuitemradio', { name: /^Roadhouse/ }).click()
  await page.getByRole('button', { name: /\. Switch website$/ }).click()
  await page.getByRole('menuitem', { name: /^All sites in Roadhouse/ }).click()
  await page.waitForURL(/\/workspaces\//)

  const rows = page.locator('#main-content').getByRole('list').getByRole('link')
  await expect(rows).toHaveCount(3)

  await expect
    .poll(
      async () => {
        await page.reload()
        await expect(rows.first()).toBeVisible()
        return rows.filter({ hasNotText: 'No status yet' }).count()
      },
      { timeout: 180_000, intervals: [5000] }
    )
    .toBe(3)

  queries.length = 0
  await page.reload()
  await expect(rows.first()).toBeVisible()
  await page.waitForLoadState('networkidle')
  expect(queries.filter((name) => name.startsWith('analytics.')), 'no analytical query').toEqual([])
  expect(queries.filter((name) => name === 'summaries.forSites')).toHaveLength(1)

  const first = rows.first()
  // A tile names the site, its views today and the pace; findings show as a count only when there are some.
  await expect(first).toHaveAccessibleName(/ \d+ views? today/)
  await expect(first.locator('svg')).toBeVisible()

  for (const scheme of ['light', 'dark'] as const) {
    for (const width of [1440, 390] as const) {
      await page.emulateMedia({ colorScheme: scheme })
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 })
      await expect(first.locator('svg')).toBeVisible()
      expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBe(0)
      if (shots) await page.screenshot({ path: join(shots, `workspace-sites-${width}-${scheme}.png`) })
    }
  }

  await page.setViewportSize({ width: 1440, height: 900 })
  await first.click()
  await page.waitForURL(/\/sites\/[^/]+$/)
})
