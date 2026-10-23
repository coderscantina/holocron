import { createHmac, randomUUID } from 'node:crypto'
import { mkdirSync } from 'node:fs'
import { createServer } from 'node:http'
import type { AddressInfo } from 'node:net'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Locator, type Page } from '@playwright/test'
import { type Account, admin, viewer, sessionFile } from './session'

/**
 * Account, team, sharing and administration journeys against the packaged server and the
 * `seed_demo` data. Set HOLOCRON_E2E_PASSWORD (admin) and HOLOCRON_E2E_VIEWER_PASSWORD. With
 * HOLOCRON_E2E_SHOTS set, screenshots at 1440 and 390 px in both schemes go to that directory.
 *
 * The journeys build on each other (the admin creates the account the 2FA journey uses), so
 * they run in order. The server allows 10 login attempts per address and minute; the file signs
 * in five times.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
if (shots) mkdirSync(shots, { recursive: true })

// Screenshot sets take four captures each; give those runs room.
test.describe.configure({ mode: 'serial', timeout: shots ? 120_000 : 30_000 })

/**
 * Waits for open and close animations, so a screenshot shows the settled state. Capped at two
 * seconds: a toast's exit animation pauses while the pointer rests on it.
 */
async function settle(page: Page) {
  await page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter((animation) => animation.effect?.getTiming().iterations !== Infinity)
          .map((animation) => animation.finished.catch(() => undefined))
      ),
      new Promise((resolve) => setTimeout(resolve, 2000)),
    ])
  )
}

/** The current page at 1440 and 390 px, light and dark. Restores 1440 px light. */
async function shotSet(page: Page, name: string) {
  if (!shots) return
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of [1440, 390]) {
      await page.setViewportSize({ width, height: 900 })
      await settle(page)
      await page.screenshot({ path: join(shots, `${name}-${width}-${scheme}.png`), fullPage: true })
    }
  }
  await page.emulateMedia({ colorScheme: 'light' })
  await page.setViewportSize({ width: 1440, height: 900 })
}

async function shot(page: Page, name: string) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`) })
}

async function signInThroughForm(page: Page, account: Account) {
  await page.goto('/login')
  await page.getByLabel('Email').fill(account.email)
  await page.getByLabel('Password').fill(account.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
}

const created = new Map<string, BrowserContextOptions['storageState']>()

/**
 * A page signed in as `account`. The admin and viewer sessions come from `global-setup.ts`;
 * accounts this file creates sign in through the form once.
 */
async function signedIn(browser: Browser, account: Account, options: BrowserContextOptions = {}) {
  let storageState = [admin, viewer].some((known) => known.email === account.email) ? sessionFile(account) : created.get(account.email)
  if (!storageState) {
    const context = await browser.newContext()
    const page = await context.newPage()
    await signInThroughForm(page, account)
    await page.waitForURL((url) => !url.pathname.startsWith('/login'))
    storageState = await context.storageState()
    created.set(account.email, storageState)
    await context.close()
  }
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: 'light', ...options, storageState })
  return context.newPage()
}

/** Answers the reauthentication dialog, if it opens, with the password. */
async function confirmPassword(page: Page, password: string) {
  const dialog = page.getByRole('dialog', { name: "Confirm it's you" })
  await expect(dialog).toBeVisible()
  await dialog.getByLabel('Password').fill(password)
  await dialog.getByRole('button', { name: 'Confirm' }).click()
  await expect(dialog).toHaveCount(0)
}

/**
 * Finishes a sensitive command: answers the password prompt if the session's check has lapsed
 * (an earlier journey may have refreshed it), then waits for `done`.
 */
async function finishSensitive(page: Page, password: string, done: Locator) {
  const dialog = page.getByRole('dialog', { name: "Confirm it's you" })
  await expect(done.or(dialog)).toBeVisible()
  if (await dialog.isVisible()) await confirmPassword(page, password)
  await expect(done).toBeVisible()
}

/** RFC 6238 code for a base32 key at the current 30-second step. */
function totp(key: string, at = Date.now()): string {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
  let bits = ''
  for (const char of key.replace(/=+$/, '').toUpperCase()) bits += alphabet.indexOf(char).toString(2).padStart(5, '0')
  const bytes = Buffer.from(bits.match(/.{8}/g)?.map((byte) => Number.parseInt(byte, 2)) ?? [])
  const counter = Buffer.alloc(8)
  counter.writeBigUInt64BE(BigInt(Math.floor(at / 30_000)))
  const digest = createHmac('sha1', bytes).update(counter).digest()
  const offset = (digest.at(-1) ?? 0) & 0x0f
  return String((digest.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).padStart(6, '0')
}

/** The value of a read-only one-time secret field. */
const secretOf = (page: Page, label: string) => page.getByRole('dialog').getByLabel(label, { exact: true }).inputValue()

/** Names are unique per run, so a rerun on the same data finds only its own rows. */
const run = Date.now().toString(36)
const keyName = `E2E export ${run}`
const teamName = `E2E Team ${run}`
const shareName = `E2E partners ${run}`
const userName = `E2E Two Factor ${run}`
const twoFactorUser = { email: `e2e-2fa-${run}@example.test`, password: '' }

const MARKER = { 'x-holocron-request': '1' }

/** A command through the page's session, as the app sends it. */
async function command<T>(page: Page, name: string, args: object): Promise<T> {
  const answer = await page.request.post(`/api/commands/${name}`, {
    headers: { ...MARKER, Origin: new URL(page.url()).origin },
    data: { idempotency_key: randomUUID(), ...args },
  })
  expect(answer.ok(), `${name}: ${await answer.text()}`).toBe(true)
  return (await answer.json()) as T
}

/** The seeded sites by name, read through the page's session. */
async function sites(page: Page): Promise<Map<string, { id: string; workspace: string }>> {
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: MARKER })
  const body = (await answer.json()) as { workspaces: Array<{ id: string; sites: Array<{ id: string; name: string }> }> }
  return new Map(body.workspaces.flatMap((w) => w.sites.map((site) => [site.name, { id: site.id, workspace: w.id }])))
}

/** isbot flags headless Chromium's own user agent; the recorder would be answered as a bot. */
const DESKTOP_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'

/**
 * Records one visit to `site` with the unchanged recorder, from a page served on another
 * loopback port, as `recordings.spec.ts` does. Turns replays on for the site first.
 */
async function recordVisit(browser: Browser, admin: Page, baseURL: string, site: string) {
  await command(admin, 'recordings.updatePolicy', {
    site,
    replay_enabled: true,
    heatmap_enabled: false,
    replay_sample_percent: 100,
    heatmap_sample_percent: 100,
    max_duration_minutes: 10,
    excluded_elements: [],
    excluded_paths: [],
  })
  const html = `<!doctype html><html><head><title>Shared shop</title></head><body>
    <main style="height:1600px;font:16px sans-serif"><h1>Shared shop</h1><button id="buy">Buy</button></main>
    <script defer src="${baseURL}/script.js" data-website-id="${site}"></script>
    <script defer src="${baseURL}/recorder.js" data-website-id="${site}"></script></body></html>`
  const shop = createServer((_, response) => response.writeHead(200, { 'content-type': 'text/html' }).end(html))
  await new Promise<void>((resolve) => shop.listen(0, '127.0.0.1', resolve))
  const { port } = shop.address() as AddressInfo
  const context = await browser.newContext({ userAgent: DESKTOP_UA })
  const visitor = await context.newPage()
  await visitor.goto(`http://127.0.0.1:${port}/`)
  await visitor.waitForFunction(() => 'umami' in window)
  await visitor.waitForTimeout(2500)
  await visitor.locator('#buy').click()
  await visitor.mouse.wheel(0, 800)
  await visitor.waitForTimeout(6500)
  await context.close()
  await new Promise<void>((resolve) => shop.close(() => resolve()))
}

/** A state the server accepts for a board card. */
const cardState = (site: string) => ({
  v: 1,
  site,
  measures: ['pageviews'],
  dimensions: ['path'],
  filter: null,
  range: { kind: 'relative', unit: 'day', count: 7 },
  timezone: 'Europe/Vienna',
  granularity: 'day',
  compare: 'none',
  metric_version: 1,
  sort: [],
  page: { size: 50, index: 0 },
  visualization: 'table',
})

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
  expect(viewer.password, 'set HOLOCRON_E2E_VIEWER_PASSWORD').not.toBe('')
})

test('profile and preferences live in the user menu and save', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/')
  await page.getByRole('button', { name: 'Account menu' }).click()
  await page.getByRole('menuitem', { name: 'Profile' }).click()
  await expect(page.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible()
  await expect(page.locator('#app-sidebar').getByRole('link', { name: 'Profile' })).toHaveCount(0)

  const name = page.getByLabel('Name', { exact: true })
  const original = await name.inputValue()
  await name.fill(`${original} E2E`)
  await page.getByRole('button', { name: 'Save' }).click()
  await expect(page.getByText('Profile saved.')).toBeVisible()
  await name.fill(original)
  await page.getByRole('button', { name: 'Save' }).click()
  await expect(name).toHaveValue(original)
  await shotSet(page, 'account-profile')

  await page.getByRole('link', { name: 'Preferences' }).click()
  await page.getByRole('combobox', { name: 'Date range' }).click()
  await page.getByRole('option', { name: 'Last 30 days' }).click()
  await expect(page.getByText('Preference saved.')).toBeVisible()
  await page.reload()
  await expect(page.getByRole('combobox', { name: 'Date range' })).toHaveText('Last 30 days')
  await shotSet(page, 'account-preferences')
  await page.context().close()
})

test('API keys: create behind the password check, shown once, revoked', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/account/api-keys')
  await page.getByRole('button', { name: 'Create key' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Name').fill(keyName)
  await dialog.getByRole('checkbox').nth(1).click()
  await shot(page, 'api-key-create')
  await dialog.getByRole('button', { name: 'Create key' }).click()
  await confirmPassword(page, admin.password)

  await expect(page.getByRole('heading', { name: 'Key created' })).toBeVisible()
  const secret = await secretOf(page, 'API key')
  expect(secret.length).toBeGreaterThan(16)
  await shot(page, 'api-key-created')
  await page.getByRole('button', { name: 'Done' }).click()

  const row = page.getByRole('row', { name: new RegExp(keyName) })
  await expect(row).toBeVisible()
  expect(await row.textContent()).not.toContain(secret)
  await shotSet(page, 'account-api-keys')
  await row.getByRole('button', { name: `Revoke ${keyName}` }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Revoke key' }).click()
  await expect(page.getByText('Key revoked.')).toBeVisible()
  await expect(row.getByText(/^Revoked/)).toBeVisible()
  await page.context().close()
})

test('admin creates an account with a one-time password', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/admin/users')
  await expect(page.getByRole('heading', { level: 1, name: 'Administration' })).toBeVisible()
  await page.getByRole('button', { name: 'Add user' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Name').fill(userName)
  await dialog.getByLabel('Email').fill(twoFactorUser.email)
  await dialog.getByRole('button', { name: 'Add user' }).click()
  await expect(page.getByRole('heading', { name: 'User added' })).toBeVisible()
  twoFactorUser.password = await secretOf(page, 'Password')
  expect(twoFactorUser.password.length).toBeGreaterThanOrEqual(12)
  await page.getByRole('button', { name: 'Done' }).click()
  await expect(page.getByRole('row', { name: new RegExp(userName) })).toBeVisible()
  await shotSet(page, 'admin-users')

  await page.getByRole('link', { name: 'Security policy' }).click()
  await expect(page.getByRole('switch', { name: 'Require two-factor authentication for everyone' })).toBeVisible()
  await shotSet(page, 'admin-security')
  await page.context().close()
})

test('2FA: enroll with the app, sign in with a recovery code, regenerate, turn off', async ({ browser }) => {
  expect(twoFactorUser.password, 'the admin journey creates this account').not.toBe('')
  const page = await signedIn(browser, twoFactorUser)
  await page.goto('/account/security')
  await page.getByRole('button', { name: 'Set up' }).click()
  const setup = page.getByRole('dialog')
  await setup.getByRole('button', { name: 'Continue' }).click()
  await expect(setup.getByRole('img', { name: 'QR code for your authenticator app' })).toBeVisible()
  const key = await setup.getByLabel('Key for manual entry').inputValue()
  await shot(page, 'two-factor-scan')
  await setup.getByLabel('Authentication code').fill(totp(key))
  await setup.getByRole('button', { name: 'Confirm' }).click()

  const codes = setup.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem')
  await expect(codes).toHaveCount(10)
  const recovery = await codes.allTextContents()
  await shot(page, 'two-factor-codes')
  await setup.getByLabel('I have kept my recovery codes').click()
  await setup.getByRole('button', { name: 'Done' }).click()
  await expect(page.getByText(/On since/)).toBeVisible()
  await shotSet(page, 'account-security')

  await page.getByRole('button', { name: 'Account menu' }).click()
  await page.getByRole('menuitem', { name: 'Sign out' }).click()
  await page.waitForURL(/\/login/)
  await signInThroughForm(page, twoFactorUser)
  await page.waitForURL(/\/login\/two-factor/)
  await page.getByRole('button', { name: 'Use a recovery code instead' }).click()
  await page.getByLabel('Recovery code').fill('0000-0000-0000-0000')
  await page.getByRole('button', { name: 'Verify' }).click()
  await expect(page.getByRole('alert')).toBeVisible()
  await shotSet(page, 'login-two-factor')
  await page.getByLabel('Recovery code').fill(recovery[0]?.trim() ?? '')
  await page.getByRole('button', { name: 'Verify' }).click()
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))

  await page.goto('/account/security')
  await expect(page.getByText('9 recovery codes left', { exact: false })).toBeVisible()
  await page.getByRole('button', { name: 'New recovery codes' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'New recovery codes' }).click()
  const prompt = page.getByRole('dialog', { name: "Confirm it's you" })
  await prompt.getByLabel('Password').fill(twoFactorUser.password)
  await prompt.getByLabel('Authentication code').fill('000000')
  await prompt.getByRole('button', { name: 'Confirm' }).click()
  await expect(prompt.getByRole('alert')).toBeVisible()
  await expect(page).toHaveURL(/\/account\/security/)
  await prompt.getByRole('button', { name: 'Use a recovery code instead' }).click()
  await prompt.getByLabel('Password').fill(twoFactorUser.password)
  await prompt.getByLabel('Recovery code').fill(recovery[1]?.trim() ?? '')
  await prompt.getByRole('button', { name: 'Confirm' }).click()
  await expect(page.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem')).toHaveCount(10)
  await page.keyboard.press('Escape')

  await page.getByRole('button', { name: 'Turn off' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Turn off' }).click()
  await expect(page.getByText('Two-factor authentication is off.')).toBeVisible()

  await page.getByRole('button', { name: 'Sign out other browsers' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Sign out other browsers' }).click()
  await expect(page.getByText(/Signed out of \d+ other browsers/)).toBeVisible()
  await page.context().close()
})

test('teams: create, invite by link, join as another person, remove, delete', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/')
  await page.getByRole('button', { name: /\. Switch workspace$/ }).click()
  await page.getByRole('menuitem', { name: 'Create team' }).click()
  await page.getByRole('dialog').getByLabel('Team name').fill(teamName)
  await page.getByRole('dialog').getByRole('button', { name: 'Create team' }).click()
  await expect(page.getByRole('heading', { level: 1, name: teamName })).toBeVisible()

  await page.getByRole('link', { name: 'Members' }).click()
  await page.getByRole('button', { name: 'Invite' }).click()
  const invite = page.getByRole('dialog')
  await invite.getByLabel('Label').fill('Viewer from e2e')
  await invite.getByRole('button', { name: 'Invite' }).click()
  const link = await secretOf(page, 'Invitation link')
  expect(link).toContain('/teams/join?code=')
  await page.getByRole('button', { name: 'Done' }).click()
  await expect(page.getByRole('row', { name: /Viewer from e2e/ }).getByText('Pending')).toBeVisible()
  await shotSet(page, 'workspace-members')

  const guest = await signedIn(browser, viewer)
  await guest.goto(link)
  await expect(guest.getByLabel('Invitation code')).not.toHaveValue('')
  await shot(guest, 'team-join')
  await guest.getByRole('button', { name: 'Join team' }).click()
  await expect(guest.getByRole('heading', { level: 1, name: teamName })).toBeVisible()
  const teamUrl = guest.url()
  await guest.goto(link)
  await guest.getByRole('button', { name: 'Join team' }).click()
  await expect(guest.getByText('This invitation is not valid.', { exact: false })).toBeVisible()

  await page.reload()
  const member = page.getByRole('row', { name: /viewer@example\.test/ })
  await expect(member).toBeVisible()
  await member.getByRole('button', { name: /^Remove / }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Remove' }).click()
  await expect(member).toHaveCount(0)

  // The removed member loses the team at their next read.
  await guest.goto(teamUrl)
  await expect(guest.getByText('This workspace is not available.')).toBeVisible()
  await guest.context().close()

  await page.getByRole('link', { name: 'Settings' }).click()
  await shotSet(page, 'workspace-settings')
  await page.getByRole('button', { name: 'Delete team' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Delete team' }).click()
  await finishSensitive(page, admin.password, page.getByText(`"${teamName}" deleted.`))
  await page.context().close()
})

test('sharing: a link shows the restricted view without a session and dies on revoke', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()
  await page.getByRole('link', { name: 'Sharing' }).click()
  await page.getByRole('button', { name: 'Create share' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByLabel('Name', { exact: true }).fill(shareName)
  await dialog.getByRole('button', { name: 'Create share' }).click()
  const link = await secretOf(page, 'Share link')
  await page.getByRole('button', { name: 'Done' }).click()
  const row = page.getByRole('row', { name: new RegExp(shareName) })
  await expect(row.getByText('Active')).toBeVisible()
  await shotSet(page, 'site-sharing')

  const anonymous = await browser.newContext({ viewport: { width: 1440, height: 900 } })
  const viewerPage = await anonymous.newPage()
  await viewerPage.goto(link)
  await expect(viewerPage.getByRole('heading', { level: 1, name: shareName })).toBeVisible()
  await expect(viewerPage.getByRole('button', { name: 'Account menu' })).toHaveCount(0)
  await expect(viewerPage.locator('#app-sidebar')).toHaveCount(0)
  await expect(viewerPage.getByRole('table')).toBeVisible()
  await shotSet(viewerPage, 'share-viewer')

  await row.getByRole('button', { name: `Revoke ${shareName}` }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Revoke share' }).click()
  await expect(row.getByText('Revoked')).toBeVisible()

  await viewerPage.reload()
  await expect(viewerPage.getByText('This link is not available')).toBeVisible()
  await shot(viewerPage, 'share-revoked')
  await anonymous.close()
  await page.context().close()
})

test('admin deletes the account and explains a refusal', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await page.goto('/admin/users')
  const row = page.getByRole('row', { name: new RegExp(userName) })
  await row.getByRole('button', { name: `Actions for ${userName}` }).click()
  await page.getByRole('menuitem', { name: 'Delete user' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Delete user' }).click()
  const refused = page.getByRole('heading', { name: "Can't delete yet" })
  // A refusal (the account owns a workspace or site) is explained instead of a bare error.
  await finishSensitive(page, admin.password, page.getByText(`${userName} deleted.`).or(refused))
  if (await refused.isVisible()) await shot(page, 'admin-user-owns-resources')
  else await expect(row).toHaveCount(0)
  await page.context().close()
})

test('shared recordings: listed and played with the grant, not found without it', async ({ browser, baseURL }) => {
  test.setTimeout(shots ? 180_000 : 90_000)
  const page = await signedIn(browser, admin)
  await page.goto('/')
  const site = (await sites(page)).get('notes.test')
  expect(site, 'seed_demo creates notes.test').toBeDefined()
  await recordVisit(browser, page, baseURL ?? '', site?.id ?? '')

  const createShare = async (name: string, recordings: boolean) => {
    await page.goto(`/sites/${site?.id}/settings/sharing`)
    await page.getByRole('button', { name: 'Create share' }).click()
    const dialog = page.getByRole('dialog')
    await dialog.getByLabel('Name', { exact: true }).fill(name)
    if (recordings) await dialog.getByRole('switch', { name: 'Watch recordings inside the share' }).click()
    await dialog.getByRole('button', { name: 'Create share' }).click()
    const link = await secretOf(page, 'Share link')
    await page.getByRole('button', { name: 'Done' }).click()
    return link
  }
  const granted = await createShare(`${shareName} replays`, true)
  const plain = await createShare(`${shareName} no replays`, false)

  const anonymous = await browser.newContext({ viewport: { width: 1440, height: 900 } })
  const viewerPage = await anonymous.newPage()
  await viewerPage.goto(granted)
  await viewerPage.getByRole('link', { name: 'Recordings' }).click()
  const play = viewerPage.getByRole('link', { name: /^Play replay recorded/ }).first()
  await expect(play).toBeVisible({ timeout: 15_000 })
  await shotSet(viewerPage, 'share-replays')
  await play.click()
  await expect(viewerPage.getByRole('button', { name: 'Play', exact: true })).toBeEnabled({ timeout: 15_000 })
  await expect(viewerPage.getByRole('button', { name: 'Save replay' })).toHaveCount(0)
  await expect(viewerPage.getByRole('link', { name: 'Other replays of this session' })).toHaveCount(0)
  await shot(viewerPage, 'share-replay-1440-light')
  const capture = new URL(viewerPage.url()).pathname.split('/').at(-1) ?? ''

  // A replay the share does not hold: a guessed ID, and a real one through a share without the grant.
  await viewerPage.goto(`${granted}/replays/00000000-0000-4000-8000-000000000000`)
  await expect(viewerPage.getByText('This replay is not available')).toBeVisible()
  await viewerPage.goto(`${plain}/replays/${capture}`)
  await expect(viewerPage.getByText('This replay is not available')).toBeVisible()
  await viewerPage.goto(`${plain}/replays`)
  await expect(viewerPage.getByText('Recordings are not part of this share')).toBeVisible()
  await viewerPage.goto(plain)
  await expect(viewerPage.getByRole('link', { name: 'Recordings' })).toHaveCount(0)
  await shot(viewerPage, 'share-no-recordings')
  await anonymous.close()
  await page.context().close()
})

test('shared board: cards inside the share render, others show as unavailable', async ({ browser }) => {
  test.setTimeout(shots ? 120_000 : 30_000)
  const page = await signedIn(browser, admin)
  await page.goto('/')
  const all = await sites(page)
  const shared = all.get('roadhouse.test')
  const other = all.get('Roadhouse shop')
  expect(shared && other, 'seed_demo creates both Roadhouse sites').toBeTruthy()
  if (!shared || !other) return

  const { board } = await command<{ board: { id: string } }>(page, 'boards.create', {
    workspace: shared.workspace,
    name: `E2E board ${run}`,
  })
  await command(page, 'boards.pin', { board: board.id, title: 'Shared pages', state: cardState(shared.id) })
  await command(page, 'boards.pin', { board: board.id, title: 'Shop pages', state: cardState(other.id) })
  const created = await command<{ secret: string }>(page, 'sharing.create', {
    name: `E2E board share ${run}`,
    target: { kind: 'board', board: board.id, sites: [shared.id] },
    export: false,
    recordings: false,
    expires_at: Date.now() + 7 * 86_400_000,
  })

  const anonymous = await browser.newContext({ viewport: { width: 1440, height: 900 } })
  const viewerPage = await anonymous.newPage()
  await viewerPage.goto(`/share/${created.secret}`)
  await expect(viewerPage.getByRole('heading', { level: 1, name: `E2E board ${run}` })).toBeVisible()
  const cards = viewerPage.getByRole('list', { name: `E2E board ${run}` }).getByRole('listitem')
  await expect(cards).toHaveCount(2)
  await expect(cards.filter({ hasText: 'Shared pages' }).getByRole('table')).toBeVisible()
  await expect(cards.filter({ hasText: 'Shop pages' }).getByText("This card's site is not part of what you can see.")).toBeVisible()
  await expect(viewerPage.getByRole('button', { name: /^Options for/ })).toHaveCount(0)
  await shotSet(viewerPage, 'share-board')
  await anonymous.close()
  await page.context().close()
})
