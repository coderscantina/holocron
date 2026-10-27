import { describe, expect, it } from 'vitest'

import type { SetupRestoreProgress } from '~/generated'

import { restoreRows } from './restore'
import { setupSteps } from './steps'

const frame = (overrides: Partial<SetupRestoreProgress>): SetupRestoreProgress => ({
  state: 'running',
  upgrade: false,
  ...overrides,
})

describe('restoreRows', () => {
  it('shows only the restore for a backup of this release', () => {
    expect(restoreRows(frame({ phase: 'restoring' }))).toEqual([{ phase: 'restoring', state: 'running' }])
    expect(restoreRows(frame({ state: 'complete' }))).toEqual([{ phase: 'restoring', state: 'done' }])
  })

  it('adds the upgrade for an older backup and moves on when the restore is done', () => {
    expect(restoreRows(frame({ phase: 'restoring', upgrade: true }))).toEqual([
      { phase: 'restoring', state: 'running' },
      { phase: 'upgrading', state: 'pending' },
    ])
    expect(restoreRows(frame({ phase: 'upgrading', upgrade: true }))).toEqual([
      { phase: 'restoring', state: 'done' },
      { phase: 'upgrading', state: 'running' },
    ])
    expect(restoreRows(frame({ state: 'complete', upgrade: true }))).toEqual([
      { phase: 'restoring', state: 'done' },
      { phase: 'upgrading', state: 'done' },
    ])
  })
})

describe('setupSteps', () => {
  it('lets a restore skip the admin and end at the sign-in', () => {
    expect(setupSteps.restore).toEqual(['token', 'check', 'path', 'backup', 'confirm', 'restore'])
  })
})
