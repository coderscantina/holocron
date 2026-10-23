import { mkdirSync } from 'node:fs'
import { dirname } from 'node:path'

import { chromium, type FullConfig } from '@playwright/test'

import { admin, sessionFile, viewer } from './session'

/**
 * Signs in once per account and stores the session, so the suite stays under the server's limit
 * of 10 sign-ins per address and minute. Journeys that test signing in use the form themselves.
 */
export default async function globalSetup(config: FullConfig) {
  const baseURL = config.projects[0]?.use.baseURL
  const browser = await chromium.launch()
  for (const account of [admin, viewer]) {
    if (account.password === '') continue
    const context = await browser.newContext({ baseURL })
    const page = await context.newPage()
    await page.goto('/login')
    await page.getByLabel(/^(Email|E-Mail)$/).fill(account.email)
    await page.getByLabel(/^(Password|Passwort)$/).fill(account.password)
    await page.getByRole('button', { name: /^(Sign in|Anmelden)$/ }).click()
    await page.waitForURL((url) => !url.pathname.startsWith('/login'))
    mkdirSync(dirname(sessionFile(account)), { recursive: true })
    await context.storageState({ path: sessionFile(account) })
    await context.close()
  }
  await browser.close()
}
