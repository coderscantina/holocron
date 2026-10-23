import { describe, expect, it } from 'vitest'

import { needsPrompt } from './reauth'

const now = 1_000_000

describe('reauthentication prompts', () => {
  it('ask before a sensitive command without a fresh check, or one about to lapse', () => {
    expect(needsPrompt('twoFactor.disable', {}, undefined, now)).toBe(true)
    expect(needsPrompt('twoFactor.disable', {}, now + 5 * 60_000, now)).toBe(false)
    expect(needsPrompt('twoFactor.disable', {}, now + 10_000, now)).toBe(true)
  })

  it('ask for role changes only when they raise someone to admin or owner', () => {
    const args = { workspace: 'w1', user: 'u1' }
    expect(needsPrompt('members.updateRole', { ...args, role: 'member' }, undefined, now)).toBe(false)
    expect(needsPrompt('members.updateRole', { ...args, role: 'viewer' }, undefined, now)).toBe(false)
    expect(needsPrompt('members.updateRole', { ...args, role: 'admin' }, undefined, now)).toBe(true)
    expect(needsPrompt('members.updateRole', { ...args, role: 'owner' }, undefined, now)).toBe(true)
  })

  it('leave second-factor enrollment to the server, which knows whether a factor would be replaced', () => {
    expect(needsPrompt('twoFactor.begin', {}, undefined, now)).toBe(false)
    expect(needsPrompt('twoFactor.confirm', { code: '123456' }, undefined, now)).toBe(false)
  })
})
