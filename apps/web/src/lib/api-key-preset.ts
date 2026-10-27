import { readAnalyticsPreset } from '~/generated/queries'

/** Whether a key's operations are exactly the read analytics preset, in any order. */
export function isReadAnalyticsPreset(operations: readonly string[]): boolean {
  return operations.length === readAnalyticsPreset.length && readAnalyticsPreset.every((op) => operations.includes(op))
}
