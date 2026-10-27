import { describe, expect, it } from 'vitest'

import { readAnalyticsPreset } from '~/generated/queries'

import { isReadAnalyticsPreset } from './api-key-preset'

describe('isReadAnalyticsPreset', () => {
  it('holds for exactly the preset operations and stops when the selection changes', () => {
    const selection: string[] = [...readAnalyticsPreset].reverse()
    expect(isReadAnalyticsPreset(selection)).toBe(true)
    expect(isReadAnalyticsPreset(selection.slice(1))).toBe(false)
    expect(isReadAnalyticsPreset([...selection, 'sessions.list'])).toBe(false)
  })
})
