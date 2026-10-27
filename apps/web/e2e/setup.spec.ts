import { type ChildProcess, spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { expect, type Page, test } from '@playwright/test'

import { admin, sessionFile } from './session'

/**
 * The setup wizard against its own server on an empty data directory, one per test, since setup
 * runs once per directory. Starts the packaged binary (`apps/server/scripts/package.sh`), or
 * HOLOCRON_E2E_BINARY. The Umami import itself is covered by the server's import tests.
 *
 * The restore specs need HOLOCRON_E2E_BACKUP_DIR, a directory with a `holocron backup` of the
 * seeded instance (scripts/e2e.sh makes one), and HOLOCRON_E2E_PASSWORD, the seeded admin's.
 * They mount the directory as the server's backup directory. The last one restores
 * HOLOCRON_E2E_OLD_BACKUP, a backup made by an older release, and is skipped without it.
 * HOLOCRON_E2E_SHOTS writes a screenshot of each wizard step at four widths in both modes.
 */
const repo = join(import.meta.dirname, '../../..')
const binary =
  process.env.HOLOCRON_E2E_BINARY ?? join(process.env.CARGO_TARGET_DIR ?? join(repo, 'target'), 'package/holocron/holocron')
const backupDir = process.env.HOLOCRON_E2E_BACKUP_DIR
const oldBackup = process.env.HOLOCRON_E2E_OLD_BACKUP
const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })
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
      ...(backupDir ? { HOLOCRON_BACKUP_DIR: backupDir } : {}),
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

/** The screen as it is, at every width and in both modes, when HOLOCRON_E2E_SHOTS is set. */
async function everyLayout(page: Page, name: string): Promise<void> {
  if (!shots) return
  await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))))
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    // The scheme switch starts color transitions; capture the settled page, not the fade.
    await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))))
    await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))))
    for (const width of [1440, 1024, 768, 390]) {
      await page.setViewportSize({ width, height: 900 })
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)
      expect(overflow, `${name} at ${width}px`).toBeLessThanOrEqual(0)
      await page.screenshot({ path: join(shots, `setup-${name}-${width}-${scheme}.png`), fullPage: true })
    }
  }
  await page.setViewportSize({ width: 1280, height: 720 })
  await page.emulateMedia({ colorScheme: 'light' })
}

/** The wrong token, then the right one, then the server check. Ends on the path step. */
async function unlock(page: Page, capture = false): Promise<void> {
  await page.goto(`${origin}/`)
  await expect(page).toHaveURL(/\/setup/)
  if (capture) await everyLayout(page, 'token')
  await page.getByLabel('Setup token').fill('not-the-token')
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByRole('alert')).toBeVisible()
  await page.getByLabel('Setup token').fill(TOKEN.toLowerCase())
  await page.getByRole('button', { name: 'Continue' }).click()
  await expect(page.getByRole('heading', { name: 'Server check' })).toBeVisible()
  if (capture) await everyLayout(page, 'check')
  await page.getByRole('button', { name: 'Continue' }).click()
  if (capture) await everyLayout(page, 'path')
}

/** Token, server check and path, then the admin. */
async function claim(page: Page, path: RegExp): Promise<void> {
  await unlock(page)
  await page.getByRole('radio', { name: path }).click()
  await page.getByRole('button', { name: 'Continue' }).click()
  await page.getByLabel('Email').fill('admin@example.test')
  await page.getByLabel('Password').fill('correct horse battery')
  await everyLayout(page, `${path.source.startsWith('Fresh') ? 'fresh' : 'import'}-admin`)
  await page.getByRole('button', { name: 'Create admin' }).click()
}

/** The first site, its tracker, and the live check turning green on a pageview. */
async function firstSite(page: Page): Promise<void> {
  await expect(page.getByRole('heading', { name: /Add your first site/ })).toBeVisible({ timeout: 60_000 })
  await page.getByLabel('Name').fill('Shop')
  await page.getByLabel('Domain').fill('shop.test')
  await everyLayout(page, 'fresh-site')
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
  await everyLayout(page, 'import-source')
  await page.getByRole('button', { name: 'Continue without import' }).click()
  await firstSite(page)
})

/** The seeded instance's sites, read with its stored session, to compare with the restored one. */
async function seededSites(page: Page): Promise<string[]> {
  const context = await page.context().browser()!.newContext({ storageState: sessionFile(admin) })
  const answer = await context.request.get(`${process.env.HOLOCRON_E2E_URL ?? 'http://127.0.0.1:8093'}/api/queries/workspaces.list`, {
    headers: { 'x-holocron-request': '1' },
  })
  const names = await siteNames(answer)
  await context.close()
  return names
}

async function siteNames(answer: { json(): Promise<unknown> }): Promise<string[]> {
  const { workspaces } = (await answer.json()) as { workspaces: Array<{ sites: Array<{ name: string }> }> }
  return workspaces.flatMap((workspace) => workspace.sites.map((site) => site.name)).sort()
}

/** The first backup the mounted backup directory lists. */
async function pickListedBackup(page: Page): Promise<void> {
  await expect(page.getByRole('heading', { name: 'Pick a backup' })).toBeVisible()
  await page.getByRole('radio').first().click()
}

/** Signs in on the login page the restore ends on, as the seeded admin. */
async function signInRestored(page: Page): Promise<void> {
  await expect(page).toHaveURL(/\/login/, { timeout: 120_000 })
  await everyLayout(page, 'signin')
  await page.getByLabel(/^Email$/).fill(admin.email)
  await page.getByLabel(/^Password$/).fill(admin.password)
  await page.getByRole('button', { name: /^Sign in$/ }).click()
  await page.waitForURL(/\/sites\//)
}

test.describe('restore a backup', () => {
  test.skip(!backupDir || !admin.password, 'set HOLOCRON_E2E_BACKUP_DIR and HOLOCRON_E2E_PASSWORD')

  test('restores the seeded instance and shows the same sites after signing in', async ({ page }) => {
    test.setTimeout(300_000)
    const expected = await seededSites(page)
    expect(expected.length).toBeGreaterThan(0)

    await unlock(page, true)
    await page.getByRole('radio', { name: /Restore a backup/ }).click()
    await page.getByRole('button', { name: 'Continue' }).click()
    await pickListedBackup(page)
    await everyLayout(page, 'restore-pick')
    await page.getByRole('button', { name: 'Continue' }).click()
    await expect(page.getByRole('heading', { name: 'Restore this backup?' })).toBeVisible()
    await everyLayout(page, 'restore-confirm')
    await page.getByRole('button', { name: 'Restore backup' }).click()
    await expect(page.getByRole('heading', { name: 'Restoring the backup' })).toBeVisible()

    await signInRestored(page)
    const answer = await page.request.get(`${origin}/api/queries/workspaces.list`, { headers: { 'x-holocron-request': '1' } })
    expect(await siteNames(answer)).toEqual(expected)
  })

  test('refuses a path that is not a backup and shows the step again', async ({ page }) => {
    await unlock(page)
    await page.getByRole('radio', { name: /Restore a backup/ }).click()
    await page.getByRole('button', { name: 'Continue' }).click()
    await page.getByLabel('Path of a backup on this server').fill(join(dataDir, 'nothing'))
    await page.getByRole('button', { name: 'Continue' }).click()
    await expect(page.getByText("Holocron can't restore this backup")).toBeVisible()
    await everyLayout(page, 'restore-refused')
  })

  test('shows its phases while it runs and the reason when it stops', async ({ page }) => {
    // The server's frames are replaced, so the running and failed screens can be captured.
    await page.route('**/api/setup/restore/start', (route) =>
      route.fulfill({ json: { state: 'running', phase: 'restoring', upgrade: true } })
    )
    const frame = (value: object) => ({
      contentType: 'text/event-stream',
      body: `data: ${JSON.stringify(value)}\n\n`,
    })
    await page.route('**/api/setup/restore/progress', (route) =>
      route.fulfill(frame({ state: 'running', phase: 'upgrading', upgrade: true }))
    )
    await unlock(page)
    await page.getByRole('radio', { name: /Restore a backup/ }).click()
    await page.getByRole('button', { name: 'Continue' }).click()
    await pickListedBackup(page)
    await page.getByRole('button', { name: 'Continue' }).click()
    await page.getByRole('button', { name: 'Restore backup' }).click()
    await expect(page.getByText('Upgrading')).toBeVisible()
    await expect(page.getByText('Restoring', { exact: true })).toBeVisible()
    await everyLayout(page, 'restore-progress')

    await page.unroute('**/api/setup/restore/progress')
    await page.route('**/api/setup/restore/progress', (route) =>
      route.fulfill(
        frame({
          state: 'failed',
          upgrade: false,
          error: { code: 'backup_corrupt', message: 'The backup does not match its manifest, so it is damaged or incomplete.' },
        })
      )
    )
    await expect(page.getByText('The restore stopped')).toBeVisible({ timeout: 10_000 })
    await everyLayout(page, 'restore-failed')
  })

  test('upgrades a backup from an older release and signs in', async ({ page }) => {
    test.skip(!oldBackup, 'set HOLOCRON_E2E_OLD_BACKUP to a backup made by an older release')
    test.setTimeout(300_000)
    await unlock(page)
    await page.getByRole('radio', { name: /Restore a backup/ }).click()
    await page.getByRole('button', { name: 'Continue' }).click()
    await page.getByLabel('Path of a backup on this server').fill(oldBackup!)
    await page.getByRole('button', { name: 'Continue' }).click()
    await expect(page.getByText(/is upgraded right after the restore/)).toBeVisible()
    await page.getByRole('button', { name: 'Restore backup' }).click()
    await expect(page.getByText('Upgrading')).toBeVisible()
    await signInRestored(page)
  })
})
