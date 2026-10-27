import { describe, expect, it } from 'vitest'

import { GLOB_MAX_BYTES, GLOB_WILDCARDS_MAX } from '~/generated/limits'

import { globError } from './glob'

describe('globError', () => {
  it('accepts a glob within the limits, counting ** as one wildcard and escapes as none', () => {
    expect(globError('/*/events/**')).toBeNull()
    expect(globError('**'.repeat(GLOB_WILDCARDS_MAX))).toBeNull()
    expect(globError('\\*'.repeat(GLOB_WILDCARDS_MAX + 1))).toBeNull()
  })

  it('refuses an empty glob and one over the byte limit, counting bytes not characters', () => {
    expect(globError('')).toBe('empty')
    expect(globError('a'.repeat(GLOB_MAX_BYTES))).toBeNull()
    expect(globError('a'.repeat(GLOB_MAX_BYTES + 1))).toBe('too_long')
    expect(globError('ä'.repeat(GLOB_MAX_BYTES / 2 + 1))).toBe('too_long')
  })

  it('refuses too many wildcards and a trailing backslash', () => {
    expect(globError('*/'.repeat(GLOB_WILDCARDS_MAX + 1))).toBe('too_many')
    expect(globError('/a\\')).toBe('trailing_escape')
    expect(globError('/a\\\\')).toBeNull()
  })
})
