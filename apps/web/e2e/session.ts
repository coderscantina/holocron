import { join } from 'node:path'

export const admin = { email: process.env.HOLOCRON_E2E_EMAIL ?? 'mike@example.test', password: process.env.HOLOCRON_E2E_PASSWORD ?? '' }
export const viewer = { email: 'viewer@example.test', password: process.env.HOLOCRON_E2E_VIEWER_PASSWORD ?? '' }

export type Account = typeof admin

/** Where `global-setup.ts` stores the signed-in browser state of `account`. */
export function sessionFile(account: Account): string {
  return join(import.meta.dirname, '.auth', `${account.email}.json`)
}
