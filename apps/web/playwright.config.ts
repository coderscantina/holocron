import { defineConfig, devices } from '@playwright/test'

/**
 * Browser journeys against a running, packaged Holocron server seeded with
 * `cargo run -p holocron-server --example seed_demo`. See docs/evidence/m03-kit-shell-and-generated-api.md.
 */
export default defineConfig({
  testDir: './e2e',
  globalSetup: './e2e/global-setup.ts',
  fullyParallel: false,
  workers: 1,
  reporter: 'list',
  use: {
    baseURL: process.env.HOLOCRON_E2E_URL ?? 'http://127.0.0.1:8093',
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
