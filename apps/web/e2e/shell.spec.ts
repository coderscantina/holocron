import { mkdirSync } from 'node:fs'
import { join } from 'node:path'

import { expect, test, type Browser, type BrowserContextOptions, type Page } from '@playwright/test'
import { type Account, admin, viewer, sessionFile } from './session'

/**
 * Shell journeys against the packaged server and the `seed_demo` data. Set
 * HOLOCRON_E2E_PASSWORD (admin) and HOLOCRON_E2E_VIEWER_PASSWORD. With HOLOCRON_E2E_SHOTS set,
 * screenshots are written to that directory.
 *
 * Journeys reuse the sessions `global-setup.ts` stores, because the server allows 10 login
 * attempts per address and minute.
 */

const shots = process.env.HOLOCRON_E2E_SHOTS
const widths = [1440, 1024, 768, 390] as const

if (shots) mkdirSync(shots, { recursive: true })

/** Waits for open and close animations, so a screenshot shows the settled state. */
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

async function shot(page: Page, name: string) {
  if (!shots) return
  await settle(page)
  await page.screenshot({ path: join(shots, `${name}.png`) })
}

async function signInThroughForm(page: Page, account: Account) {
  await page.goto('/login')
  await page.getByLabel(/^(Email|E-Mail)$/).fill(account.email)
  await page.getByLabel(/^(Password|Passwort)$/).fill(account.password)
  await page.getByRole('button', { name: /^(Sign in|Anmelden)$/ }).click()
  await page.waitForURL(/\/sites\//)
}

/** A page signed in as `account`, in a fresh context with the given options. */
async function signedIn(browser: Browser, account: Account = admin, options: BrowserContextOptions = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...options, storageState: sessionFile(account) })
  const page = await context.newPage()
  await page.goto('/')
  await page.waitForURL(/\/sites\//)
  return page
}

/** Opens a site through the header selectors, as a person would. */
async function openSite(page: Page, workspace: string, site: string) {
  await page.getByRole('button', { name: /\. (Switch workspace|Arbeitsbereich wechseln)$/ }).click()
  await page.getByRole('menuitemradio', { name: new RegExp(`^${workspace}`) }).click()
  await page.getByRole('button', { name: /\. (Switch website|Website wechseln)$/ }).click()
  await page.getByRole('menuitemradio', { name: new RegExp(`^${site.replace('.', '\\.')}`) }).click()
  await expect(page.getByRole('button', { name: new RegExp(`^${site}.*\\. (Switch website|Website wechseln)$`) })).toBeVisible()
}

/** The overview opens on today's pace; the measures, breakdowns and findings sit behind this switch. */
async function showOverviewDetails(page: Page, label = 'Overview view') {
  await page.getByRole('radiogroup', { name: label }).getByRole('radio', { name: 'Details' }).click()
}

/** Neither the page nor any visible element inside the main region reaches past the viewport. */
async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const page = document.documentElement.scrollWidth - window.innerWidth
    const clipped = Array.from(document.querySelectorAll('#main-content *')).filter((element) => {
      const box = element.getBoundingClientRect()
      return box.width > 0 && box.right > window.innerWidth + 1 && !element.closest('.sr-only')
    })
    return { page, clipped: clipped.slice(0, 3).map((element) => element.outerHTML.slice(0, 120)) }
  })
  expect(overflow).toEqual({ page: 0, clipped: [] })
}

test.beforeAll(() => {
  expect(admin.password, 'set HOLOCRON_E2E_PASSWORD').not.toBe('')
})

test('sign-in refuses a wrong password, then signs in', async ({ page }) => {
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    await page.goto('/login')
    await page.getByLabel('Email').fill(admin.email)
    await page.getByLabel('Password').fill('not the password')
    await page.getByRole('button', { name: 'Sign in' }).click()
    await expect(page.getByText('Email or password is wrong.')).toBeVisible()
    await shot(page, `login-error-${scheme}`)
  }
  await signInThroughForm(page, admin)
  await expect(page.getByRole('button', { name: 'Account menu' })).toBeVisible()
})

test('the overview of a site opens at most one live stream per topic', async ({ browser }) => {
  const page = await signedIn(browser)
  const streams: string[] = []
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.pathname.endsWith('/subscribe')) streams.push(url.searchParams.get('topic') ?? '')
  })
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  await showOverviewDetails(page)
  await expect(page.getByRole('region', { name: 'Summary measures' })).toBeVisible()
  await expect(page.getByText('All figures corrected')).toBeVisible()

  expect(streams.length).toBeGreaterThan(0)
  expect(new Set(streams).size).toBe(streams.length)
  await page.context().close()
})

test('overview, context switching and menus at every width in both modes', async ({ browser }) => {
  const page = await signedIn(browser)
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  await showOverviewDetails(page)
  await expect(page.getByRole('region', { name: 'Summary measures' })).toBeVisible()
  await expect(page.getByText('All figures corrected')).toBeVisible()

  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme })
    for (const width of widths) {
      await page.setViewportSize({ width, height: 900 })
      await expectNoHorizontalOverflow(page)
      await shot(page, `shell-overview-${width}-${scheme}`)
    }
  }

  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: 'Account menu' }).click()
  await expect(page.getByRole('menuitem', { name: 'Sign out' })).toBeVisible()
  await shot(page, 'user-menu-dark')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('button', { name: 'Account menu' })).toBeFocused()

  await page.getByRole('button', { name: /\. Switch website$/ }).click()
  await expect(page.getByRole('menuitemradio', { name: /^roadhouse\.at/ })).toHaveAttribute('aria-checked', 'true')
  await shot(page, 'site-selector-dark')
  await page.keyboard.press('Escape')

  await page.getByRole('button', { name: /\. Switch workspace$/ }).click()
  await shot(page, 'workspace-selector-dark')
  await page.keyboard.press('Escape')

  // The test browser reports a Windows user agent, so the shortcut is Ctrl+K.
  await page.keyboard.press('Control+k')
  await expect(page.getByPlaceholder('Search pages, websites and actions')).toBeFocused()
  await page.keyboard.type('Roadhouse shop')
  await expect(page.getByRole('option', { name: /Roadhouse shop/ })).toHaveAttribute('data-highlighted', '')
  await shot(page, 'command-palette-dark')
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: /^Roadhouse shop.*\. Switch website$/ })).toBeVisible()
  await page.context().close()
})

test('the canvas is one inset panel under a borderless top bar that stays one row', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  await page.evaluate(() => {
    document.documentElement.dataset.density = 'comfortable'
  })
  const header = page.getByRole('banner')
  const canvas = page.locator('#main-content')

  for (const width of [1440, 1024, 768]) {
    await page.setViewportSize({ width, height: 900 })
    const bar = await header.boundingBox()
    const panel = await canvas.boundingBox()
    expect(bar?.height, `top bar at ${width}px`).toBe(56)
    if (width < 1024) continue
    expect(panel?.y, 'the canvas starts under the top bar').toBeGreaterThanOrEqual((bar?.y ?? 0) + (bar?.height ?? 0))
    expect(width - ((panel?.x ?? 0) + (panel?.width ?? 0)), 'inset from the right frame edge').toBe(10)
    expect(900 - ((panel?.y ?? 0) + (panel?.height ?? 0)), 'inset from the bottom frame edge').toBe(10)
  }

  await page.setViewportSize({ width: 1440, height: 900 })
  await expect(header).toHaveCSS('border-bottom-width', '0px')
  await expect(canvas).toHaveCSS('border-top-left-radius', '18px')
  await expect(header.getByRole('button', { name: /\. Switch workspace$/ })).toHaveCSS('height', '38px')
  await expect(header.getByRole('button', { name: /\. Switch website$/ })).toHaveCSS('border-top-left-radius', '10px')
  await expect(header.getByRole('button', { name: 'Account menu' })).toHaveCSS('width', '38px')

  const title = canvas.getByRole('heading', { level: 1, name: 'Overview' })
  await expect(title).toBeVisible()
  await expect(title.locator('xpath=..')).toHaveCSS('border-bottom-width', '1px')
  await page.context().close()
})

test('the sidebar carries no account navigation and collapses to a rail', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  const sidebar = page.locator('#app-sidebar')
  await expect(sidebar.getByRole('link', { name: 'Overview' })).toBeVisible()
  await expect(sidebar.getByText(admin.email)).toHaveCount(0)
  await expect(sidebar.getByRole('link', { name: 'Account' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Toggle sidebar' }).click()
  await expect(sidebar).toHaveCSS('width', '56px')
  await shot(page, 'sidebar-collapsed-light')
  await page.getByRole('button', { name: 'Toggle sidebar' }).click()
  await page.context().close()
})

test('the sidebar groups the site routes, with the jump field under the brand', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.evaluate(() => {
    document.documentElement.dataset.density = 'comfortable'
  })
  const sidebar = page.locator('#app-sidebar')
  const groups = {
    Workspace: ['Boards'],
    Traffic: ['Overview', 'Explore', 'Realtime', 'Sessions'],
    Behaviour: ['Events', 'Funnels', 'Goals', 'Journeys', 'Retention'],
    Acquisition: ['Campaigns', 'Attribution', 'Revenue'],
    Experience: ['Performance', 'Replays', 'Heatmaps'],
  }
  await expect(sidebar.getByRole('heading', { level: 2 })).toHaveText(Object.keys(groups))
  const links = sidebar.getByRole('navigation').getByRole('link')
  await expect(links).toHaveText([...Object.values(groups).flat(), 'Website settings'].map((name) => new RegExp(`^${name}`)))
  await expect(sidebar.getByRole('link', { name: 'Overview' })).toHaveClass(/(^| )bg-nav-active!( |$)/)
  await expect(sidebar).toHaveCSS('border-right-width', '0px')
  await expect(page.getByRole('banner').getByRole('button', { name: 'Open command palette' })).toHaveCount(0)

  const jump = sidebar.getByRole('button', { name: /^Jump to anything/ })
  await expect(jump).toHaveCSS('height', '44px')
  await expect(jump).toContainText(/Ctrl K|⌘K/)
  await shot(page, 'nav-sidebar-1440-dark')

  await jump.click()
  await expect(page.getByPlaceholder('Search pages, websites and actions')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(jump).toBeFocused()

  await expect(sidebar.getByRole('link', { name: /^Realtime/ })).toHaveAccessibleName(/^Realtime ?, \d+ visitors? active now$/, { timeout: 15_000 })
  await shot(page, 'nav-sidebar-live-1440-dark')
  await page.emulateMedia({ colorScheme: 'light' })
  await shot(page, 'nav-sidebar-live-1440-light')
  await page.context().close()
})

test('the collapsed rail keeps every route and turns the jump field into an icon button', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  const sidebar = page.locator('#app-sidebar')
  await page.getByRole('button', { name: 'Toggle sidebar' }).click()
  await expect(sidebar).toHaveCSS('width', '56px')
  await expect(sidebar.getByRole('heading')).toHaveCount(0)
  await expect(sidebar.getByRole('link')).toHaveCount(18)
  const jump = sidebar.getByRole('button', { name: 'Open command palette' })
  await expect(jump).toBeVisible()
  await shot(page, 'nav-rail-1440-light')
  await jump.click()
  await expect(page.getByPlaceholder('Search pages, websites and actions')).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(jump).toBeFocused()
  await page.getByRole('button', { name: 'Toggle sidebar' }).click()
  await page.context().close()
})

test('the phone drawer shows the jump field and the groups, the tabs stay three', async ({ browser }) => {
  for (const scheme of ['light', 'dark'] as const) {
    const page = await signedIn(browser, admin, { viewport: { width: 390, height: 844 }, colorScheme: scheme })
    await openSite(page, 'Roadhouse', 'roadhouse.test')
    const tabs = page.getByRole('navigation', { name: 'Main navigation' })
    await expect(tabs.getByRole('link')).toHaveText(['Overview', 'Explore', 'Realtime'])
    await tabs.getByRole('button', { name: 'Menu' }).click()
    const drawer = page.getByRole('dialog')
    await expect(drawer.getByRole('heading', { level: 2 })).toHaveText(['Analytics navigation', 'Workspace', 'Traffic', 'Behaviour', 'Acquisition', 'Experience'])
    await shot(page, `nav-drawer-390-${scheme}`)
    await drawer.getByRole('button', { name: /^Jump to anything/ }).click()
    await expect(page.getByPlaceholder('Search pages, websites and actions')).toBeFocused()
    await page.context().close()
  }
})

test('the site menu groups sites by base domain and scrolls inside the menu', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const suffixes = ['example.com', 'acme.co.uk', 'bergbahn.at', 'shop.example.de', 'verein.or.at', 'studio.io']
  await page.route(/workspaces\.list/, async (route) => {
    const response = await route.fetch()
    const body = await response.json()
    const workspace = body.workspaces.find((candidate: { name: string }) => candidate.name === 'Roadhouse')
    for (let index = 0; index < 40; index++) {
      const id = `00000000-0000-4000-8000-${String(index).padStart(12, '0')}`
      workspace.sites.push({ id, name: `Site ${index}`, domain: `s${index}.${suffixes[index % suffixes.length]}` })
    }
    await route.fulfill({ response, json: body })
  })
  await page.reload()
  await page.waitForURL(/\/sites\//)
  await openSite(page, 'Roadhouse', 'roadhouse.test')

  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 })
    await page.getByRole('button', { name: /\. Switch website$/ }).click()
    const menu = page.getByRole('menu')
    await expect(menu.getByRole('menuitem', { name: /^All sites in Roadhouse/ })).toBeVisible()
    await expect(menu.getByText('acme.co.uk', { exact: true })).toBeVisible()
    const box = await menu.boundingBox()
    expect(box?.y).toBeGreaterThanOrEqual(0)
    expect((box?.y ?? 0) + (box?.height ?? 0)).toBeLessThanOrEqual(page.viewportSize()?.height ?? 0)
    const scrolls = await menu.evaluate((element) => element.scrollHeight > element.clientHeight)
    expect(scrolls, `menu scrolls at ${width}px`).toBe(true)
    await shot(page, `nav-site-menu-${width}-dark`)
    await page.emulateMedia({ colorScheme: 'light' })
    await shot(page, `nav-site-menu-${width}-light`)
    await page.emulateMedia({ colorScheme: 'dark' })
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    await expect(menu.locator('[data-highlighted]')).toHaveCount(1)
    await page.keyboard.press('Escape')
    await expect(page.getByRole('button', { name: /\. Switch website$/ })).toBeFocused()
  }
  await page.context().close()
})

test('the site chip and menu show each site pace from the summaries and ask for nothing else', async ({ browser }) => {
  test.setTimeout(240_000)
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  const queries: string[] = []
  let summarized = false
  page.on('request', (request) => {
    const name = /\/queries\/([\w.]+)/.exec(request.url())?.[1]
    if (name) queries.push(name)
  })
  page.on('response', async (response) => {
    if (!response.url().includes('/queries/summaries.forSites')) return
    const body: { summaries: unknown[] } = await response.json()
    if (body.summaries.length > 0) summarized = true
  })
  await openSite(page, 'Roadhouse', 'roadhouse.test')

  // The background step writes the first summaries within a round; the stream pushes them to the open page.
  await expect.poll(() => summarized, { timeout: 180_000, intervals: [1000] }).toBe(true)

  const chip = page.getByRole('button', { name: /\. Switch website$/ })
  await expect(chip).toHaveAccessibleName(/^[^,]+\. Switch website$/)
  // The chip words the pace only once the site has a typical weekday; a new site keeps its name alone.
  await expect(chip).toHaveAccessibleDescription(/^$|typical|usual range/)
  const row = page.getByRole('menuitemradio', { name: /^roadhouse\.at/ })
  for (const width of [1440, 390]) {
    for (const scheme of ['dark', 'light'] as const) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 })
      await page.emulateMedia({ colorScheme: scheme })
      queries.length = 0
      await chip.click()
      await expect(row).toBeVisible()
      await expect(row).toContainText(/(vs typical|usual range|new site|views? today)(\d+ findings?)?$/)
      await page.waitForTimeout(500)
      expect(queries.filter((name) => name !== 'summaries.forSites'), 'no other query').toEqual([])
      expect(queries.length, 'at most one refetch').toBeLessThanOrEqual(1)
      await shot(page, `pace-site-menu-${width}-${scheme}`)
      await page.keyboard.press('Escape')
      await shot(page, `pace-site-chip-${width}-${scheme}`)
    }
  }
  await page.context().close()
})

test('keyboard: skip link, visible focus and route focus', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  await page.keyboard.press('Tab')
  await expect(page.getByRole('link', { name: 'Skip to content' })).toBeFocused()
  await shot(page, 'skip-link-light')
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await shot(page, 'focus-ring-light')

  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()
  await expect(page.locator('#main-content')).toBeFocused()
  await page.context().close()
})

test('phone navigation: tab bar and drawer reach the same routes', async ({ browser }) => {
  for (const scheme of ['light', 'dark'] as const) {
    const page = await signedIn(browser, admin, { viewport: { width: 390, height: 844 }, colorScheme: scheme })
    const tabs = page.getByRole('navigation', { name: 'Main navigation' })
    await expect(tabs.getByRole('link', { name: 'Overview' })).toBeVisible()
    await tabs.getByRole('button', { name: 'Menu' }).click()
    const drawer = page.getByRole('dialog')
    await expect(drawer.getByRole('link', { name: 'Website settings' })).toBeVisible()
    await shot(page, `mobile-drawer-${scheme}`)
    await drawer.getByRole('link', { name: 'Website settings' }).click()
    await expect(page.getByRole('dialog')).toHaveCount(0)
    await expect(page.getByRole('heading', { level: 1, name: 'Website settings' })).toBeVisible()
    await expectNoHorizontalOverflow(page)
    await shot(page, `mobile-settings-${scheme}`)

    await page.getByRole('button', { name: 'Account menu' }).click()
    await expect(page.getByRole('menuitem', { name: 'Sign out' })).toBeVisible()
    await shot(page, `mobile-user-menu-${scheme}`)
    await page.context().close()
  }
})

test('rename a site through the generated resources, with validation and stale revisions', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  await openSite(page, 'Roadhouse', 'Roadhouse shop')
  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()

  const name = page.getByLabel('Name', { exact: true })
  // The settings page has more status regions (policies, recalculation); this is the site section's.
  const saved = page.getByRole('region', { name: 'Name and domain' }).getByRole('status')
  await name.fill('   ')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.getByText('Required.')).toBeVisible()
  await shot(page, 'settings-field-error-light')

  await name.fill('Roadhouse online shop')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(saved).toHaveText('Saved.')
  await expect(page.getByRole('button', { name: /^Roadhouse online shop.*\. Switch website$/ })).toBeVisible()
  await shot(page, 'settings-saved-light')

  // A second window of the same browser, as a person has it, reads the site now and then holds
  // on to that revision.
  const other = await page.context().newPage()
  await other.goto(page.url())
  await expect(other.getByLabel('Name', { exact: true })).toHaveValue('Roadhouse online shop')

  await name.fill('Roadhouse shop')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(saved).toHaveText('Saved.')

  await other.getByLabel('Domain', { exact: true }).fill('store.roadhouse.test')
  await other.getByRole('button', { name: 'Save changes' }).click()
  await expect(other.getByText(/Someone changed this since you opened it/)).toBeVisible()
  await shot(other, 'settings-stale-light')
  await other.getByRole('button', { name: 'Reload settings' }).click()
  await expect(other.getByLabel('Name', { exact: true })).toHaveValue('Roadhouse shop')
  await expect(other.getByLabel('Domain', { exact: true })).toHaveValue('shop.roadhouse.test')
  await page.context().close()
})

test('a guessed site ID answers like a missing site', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  await page.goto('/sites/00000000-0000-4000-8000-000000000000')
  await expect(page.getByText('Website not available')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Choose a workspace' })).toBeVisible()
  await shot(page, 'site-unavailable-dark')
  await page.context().close()
})

test('a viewer sees settings read-only', async ({ browser }) => {
  test.skip(viewer.password === '', 'set HOLOCRON_E2E_VIEWER_PASSWORD')
  const page = await signedIn(browser, viewer, { colorScheme: 'light' })
  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()
  await expect(page.getByText('Your role can see these settings but not change them.')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Save changes' })).toHaveCount(0)
  await shot(page, 'settings-viewer-light')
  await page.context().close()
})

test('session and identity policies: validation, recalculation and consent revocation', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  await openSite(page, 'Roadhouse', 'roadhouse.test')
  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()
  const sections = page.getByRole('navigation', { name: 'Settings sections' })

  await sections.getByRole('link', { name: 'Sessions' }).click()
  await expect(page).toHaveURL(/\/settings\/sessions$/)
  const inactivity = page.getByLabel('Inactivity window in minutes')
  await inactivity.fill('0')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.locator('#session-window-error')).toBeVisible()
  await inactivity.fill('45')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.getByRole('status').first()).toHaveText('Saved.')

  await page.getByRole('button', { name: 'Recalculate' }).click()
  await expect(page.getByText(/^Recalculated \d{4}-\d{2}-\d{2} to \d{4}-\d{2}-\d{2}\.$/)).toBeVisible({ timeout: 15_000 })
  await shot(page, 'settings-sessions-light')
  await inactivity.fill('30')
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.getByRole('status').first()).toHaveText('Saved.')

  await sections.getByRole('link', { name: 'Identity' }).click()
  const persistent = page.getByLabel('Link consented visitors to persistent profiles')
  await expect(persistent).not.toBeChecked()
  await persistent.check()
  await page.getByRole('button', { name: 'Save changes' }).click()
  await expect(page.getByRole('status')).toHaveText('Saved.')

  await persistent.uncheck()
  await expect(page.getByText(/^Turning this off revokes every consent now/)).toBeVisible()
  await shot(page, 'settings-identity-revoke-light')
  await page.getByRole('button', { name: 'Turn off and revoke consents' }).click()
  await expect(page.getByRole('status')).toHaveText('Saved.')
  await expect(persistent).not.toBeChecked()

  // The sidebar keeps "Website settings" current on every settings section.
  await expect(page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' })).toHaveClass(/(^| )bg-nav-active!( |$)/)
  await page.context().close()
})

test('a viewer cannot read the identity policy', async ({ browser }) => {
  test.skip(viewer.password === '', 'set HOLOCRON_E2E_VIEWER_PASSWORD')
  const page = await signedIn(browser, viewer, { colorScheme: 'dark' })
  await page.locator('#app-sidebar').getByRole('link', { name: 'Website settings' }).click()
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('link', { name: 'Sessions' }).click()
  await expect(page.getByRole('button', { name: 'Recalculate' })).toHaveCount(0)
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('link', { name: 'Identity' }).click()
  await expect(page.getByText('Only roles that can configure this website see its identity policy.')).toBeVisible()
  await shot(page, 'settings-identity-viewer-dark')
  await page.context().close()
})

test('German copy fits at desktop and phone widths', async ({ browser }) => {
  const page = await signedIn(browser, admin, { locale: 'de-DE', colorScheme: 'dark' })
  await openSite(page, 'Roadhouse', 'Roadhouse Konzertagentur und Veranstaltungstechnik Salzburg')
  await showOverviewDetails(page, 'Ansicht der Übersicht')
  await expect(page.getByRole('region', { name: 'Kennzahlen' })).toBeVisible()
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 })
    await expectNoHorizontalOverflow(page)
    await shot(page, `overview-de-${width}-dark`)
  }
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.getByRole('button', { name: 'Kontomenü' }).click()
  await page.getByRole('menuitem', { name: 'Darstellung' }).hover()
  await shot(page, 'user-menu-de-dark')
  await page.context().close()
})

test('the installation name comes from the server at runtime', async ({ browser }) => {
  const expected = process.env.HOLOCRON_E2E_INSTANCE_NAME
  test.skip(!expected, 'restart the server with HOLOCRON_INSTANCE_NAME and set HOLOCRON_E2E_INSTANCE_NAME')
  const page = await signedIn(browser, admin, { colorScheme: 'light' })
  await expect(page.locator('#app-sidebar').getByRole('link', { name: expected })).toBeVisible()
  await expect(page).toHaveTitle(new RegExp(`· ${expected}$`))
  await shot(page, 'runtime-config-instance-name-light')
  await page.context().close()
})

/**
 * Findings are mocked: seeded traffic may not trigger any. The summaries of the first two
 * sites of the workspace are patched to carry findings, the second one stale.
 */
async function mockFindingMarks(page: Page, options: { stale?: boolean } = {}) {
  const answer = await page.request.get('/api/queries/workspaces.list', { headers: { 'x-holocron-request': '1' } })
  const { workspaces }: { workspaces: Array<{ sites: Array<{ id: string; name: string }> }> } = await answer.json()
  const siblings = workspaces.find((workspace) => workspace.sites.some((site) => site.name === 'roadhouse.test'))?.sites ?? []
  const roadhouse = siblings.find((site) => site.name === 'roadhouse.test')
  const other = siblings.find((site) => site !== roadhouse)
  if (!roadhouse || !other) throw new Error('seed_demo did not run')

  await page.route('**/api/queries/summaries.forSites*', async (route) => {
    const response = await route.fetch()
    const body: { summaries: Array<{ site: string; stale: boolean; finding_count: number }> } = await response.json()
    const summaries = body.summaries.map((summary) =>
      summary.site === roadhouse.id
        ? { ...summary, stale: options.stale ?? false, finding_count: 4 }
        : summary.site === other.id
          ? { ...summary, stale: true, finding_count: 5 }
          : summary
    )
    await route.fulfill({ response, json: { ...body, summaries } })
  })

  const now = Date.now()
  const state = {
    v: 1,
    site: roadhouse.id,
    measures: ['visitor_days'],
    dimensions: [],
    filter: null,
    range: { kind: 'relative', unit: 'day', count: 1 },
    timezone: 'Europe/Vienna',
    granularity: 'hour',
    compare: { kind: 'typical_weekday' },
    metric_version: 1,
    sort: [],
    page: { size: 25, index: 0 },
    visualization: 'line',
  }
  const base = { computed_at: now, window_start: now - 6 * 3_600_000, share: 0, rule_version: 1, state }
  const goal = (rank: number, today: number) => ({
    ...base,
    rank,
    rule: 'goal_pace',
    values: { kind: 'goal_pace', goal: 'g1', goal_version: 1, today, typical: 10, relative_change: (today - 10) / 10 },
  })
  await page.route('**/api/queries/summaries.findings*', (route) =>
    route.fulfill({
      json: {
        site: roadhouse.id,
        computed_at: now,
        watermark_batch: 1,
        watermark_sequence: 1,
        stale: options.stale ?? false,
        findings: [
          goal(1, 5),
          goal(2, 3),
          {
            ...base,
            rank: 3,
            rule: 'funnel_step',
            values: {
              kind: 'funnel_step',
              card: 'card1',
              title: 'Ticket checkout',
              step: 2,
              until_hour: 14,
              entered: 214,
              dropped: 86,
              baseline_entered: 1000,
              baseline_dropped: 310,
              baseline_days: 8,
              drop_off: 0.4,
              baseline_drop_off: 0.31,
            },
          },
          {
            ...base,
            rank: 4,
            rule: 'performance',
            values: {
              kind: 'performance',
              metric: 'lcp',
              device: { t: 'string', v: 'mobile' },
              value: { t: 'millis', v: 3400 },
              observations: 1420,
              slowest: { t: 'millis', v: 2600 },
              slowest_day: '2026-10-01',
              days: 28,
            },
          },
        ],
      },
    })
  )
}

test('findings badge the report items they belong to and mark the site in the menu', async ({ browser }) => {
  const page = await signedIn(browser, admin, { colorScheme: 'dark' })
  await mockFindingMarks(page)
  await openSite(page, 'Roadhouse', 'roadhouse.test')

  const sidebar = page.locator('#app-sidebar')
  await expect(sidebar.getByRole('link', { name: /^Goals ?, 2 findings$/ })).toBeVisible()
  await expect(sidebar.getByRole('link', { name: /^Funnels ?, 1 finding$/ })).toBeVisible()
  await expect(sidebar.getByRole('link', { name: /^Performance ?, 1 finding$/ })).toBeVisible()
  await expect(sidebar.getByRole('link', { name: 'Overview' })).toHaveAccessibleName('Overview')
  await shot(page, 'findings-nav-badges-1440-dark')

  await page.getByRole('button', { name: 'Toggle sidebar' }).click()
  await expect(sidebar).toHaveCSS('width', '56px')
  await expect(sidebar.getByRole('link', { name: /^Goals ?, 2 findings$/ })).toBeVisible()
  await shot(page, 'findings-nav-rail-dot-1440-dark')
  await page.getByRole('button', { name: 'Toggle sidebar' }).click()

  await page.getByRole('button', { name: /\. Switch website$/ }).click()
  await expect(page.getByRole('menuitemradio', { name: /^roadhouse\.at.*4 findings$/ })).toBeVisible()
  await expect(page.getByRole('menuitemradio', { name: /findings?$/ })).toHaveCount(1)
  await shot(page, 'findings-site-menu-dot-1440-dark')
  await page.context().close()
})

test('stale findings badge nothing', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await mockFindingMarks(page, { stale: true })
  await openSite(page, 'Roadhouse', 'roadhouse.test')

  const sidebar = page.locator('#app-sidebar')
  await expect(sidebar.getByRole('link', { name: 'Goals' })).toHaveAccessibleName('Goals')
  await expect(sidebar.getByRole('link', { name: /findings?$/ })).toHaveCount(0)
  await page.getByRole('button', { name: /\. Switch website$/ }).click()
  await expect(page.getByRole('menuitemradio', { name: /^roadhouse\.at/ })).toBeVisible()
  await expect(page.getByRole('menuitemradio', { name: /findings?$/ })).toHaveCount(0)
  await page.context().close()
})

test('the palette lists the site findings and opens the one chosen on its report', async ({ browser }) => {
  const page = await signedIn(browser, admin)
  await mockFindingMarks(page)
  await openSite(page, 'Roadhouse', 'roadhouse.test')

  await page.keyboard.press('Control+k')
  const group = page.getByRole('group', { name: 'Findings' })
  await expect(group.getByRole('option')).toHaveCount(4)
  await expect(group.getByRole('option', { name: /step 2/i })).toBeVisible()
  await group.getByRole('option', { name: /step 2/i }).click()
  await expect(page).toHaveURL(/\/funnels\?s=/)
  await expect(page.getByPlaceholder('Search pages, websites and actions')).toHaveCount(0)
  await page.context().close()
})
