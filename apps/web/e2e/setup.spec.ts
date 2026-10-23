import { type ChildProcess, spawn } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { expect, type Page, test } from '@playwright/test'

/**
 * The setup wizard against its own server on an empty data directory, one per test, since setup
 * runs once per directory. Starts the packaged binary (`apps/server/scripts/package.sh`), or
 * HOLOCRON_E2E_BINARY. The Umami import itself is covered by the server's import tests.
 */
const repo = join(import.meta.dirname, '../../..')
const binary =
  process.env.HOLOCRON_E2E_BINARY ?? join(process.env.CARGO_TARGET_DIR ?? join(repo, 'target'), 'package/holocron/holocron')
const TOKEN = 'E2E2-SETU-PTOK-EN22'
const DESKTOP_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36'

let server: ChildProcess
let dataDir: string
let origin: string

async function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const probe = createServer()
    probe.once('error', reject)
    probe.listen(0, '127.0.0.1', () => {
      const address = probe.address()
      probe.close(() => (typeof address === 'object' && address ? resolve(address.port) : reject(new Error('no port'))))
    })
  })
}

test.beforeEach(async ({ request }) => {
  dataDir = mkdtempSync(join(tmpdir(), 'holocron-e2e-setup-'))
  origin = `http://127.0.0.1:${await freePort()}`
  server = spawn(binary, ['serve', '--data-dir', join(dataDir, 'data')], {
    env: {
      ...process.env,
      HOLOCRON_BIND: origin.slice('http://'.length),
      HOLOCRON_PUBLIC_URL: origin,
      HOLOCRON_SETUP_TOKEN: TOKEN,
      HOLOCRON_DISK_RESERVE_MIB: '64',
    },
    stdio: 'ignore',
  })
  await expect
    .poll(() => request.get(`${origin}/health`).then((response) => response.ok(), () => false), { timeout: 30_000 })
    .toBe(true)
})

test.afterEach(async () => {
  const exited = new Promise((resolve) => server.once('exit', resolve))
  server.kill('SIGTERM')
  await exited
  rmSync(dataDir, { recursive: true, force: true })
})

/** Token, server check and path, then the admin. */
async function claim(page: Page, path: RegExp): Promise<void> {
  await page.goto(`${origin}/`)
  await expect(page).toHaveURL(/\/setup/)
  await page.getByLabel('Setup token').fill('not-the-token')
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByRole('alert')).toBeVisible()
  await page.getByLabel('Setup token').fill(TOKEN.toLowerCase())
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByRole('heading', { name: 'Server check' })).toBeVisible()
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByRole('radio', { name: path }).click()
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Email').fill('admin@example.test')
  await page.getByLabel('Password').fill('correct horse battery')
  await page.getByRole('button', { name: 'Create admin' }).click()
}

/** The first site, its tracker, and the live check turning green on a pageview. */
async function firstSite(page: Page): Promise<void> {
  await expect(page.getByRole('heading', { name: /Add your first site/ })).toBeVisible({ timeout: 60_000 })
  await page.getByLabel('Name').fill('Shop')
  await page.getByLabel('Domain').fill('shop.test')
  await page.getByRole('button', { name: 'Add site' }).click()
  const tracker = page.getByLabel('Tracking code')
  await expect(tracker).toHaveValue(/data-website-id/)
  const site = /data-website-id="([^"]+)"/.exec(await tracker.inputValue())?.[1]
  expect(site).toBeTruthy()

  const visitor = await page.context().browser()!.newContext({ userAgent: DESKTOP_UA })
  const sent = await visitor.request.post(`${origin}/api/send`, {
    data: { type: 'event', payload: { website: site, url: '/welcome', hostname: 'shop.test' } },
  })
  expect(sent.ok()).toBe(true)
  await visitor.close()
  await expect(page.getByText('First pageview arrived: /welcome')).toBeVisible({ timeout: 30_000 })
}

test('a fresh start reaches a site with its first pageview, without a shell', async ({ page, request }) => {
  test.setTimeout(120_000)
  const closed = await request.post(`${origin}/api/send`, { data: {} })
  expect(closed.status()).toBe(503)
  await claim(page, /Fresh start/)
  await firstSite(page)
})

test('the import path can continue without an import', async ({ page }) => {
  test.setTimeout(120_000)
  await claim(page, /Import from Umami/)
  await expect(page.getByRole('heading', { name: /Connect to Umami/ })).toBeVisible()
  await page.getByRole('button', { name: 'Continue without import' }).click()
  await firstSite(page)
})
