import { describe, expect, it } from 'vitest'

import { ApiError } from '~/api'

import { messageFor } from './useErrorMessage'

const t = (key: string, named?: Record<string, unknown>) => `${key} ${JSON.stringify(named ?? {})}`

const refusal = (expected_origin?: string) =>
  new ApiError(403, { error_code: 'origin_rejected', message: '', expected_origin }, new Response())

describe('messageFor', () => {
  it('tells a refused origin where to open Holocron when the server names it', () => {
    expect(messageFor(refusal('http://127.0.0.1:8100'), t)).toBe(
      'errors.origin_rejected_at {"origin":"http://127.0.0.1:8100"}'
    )
  })

  it('keeps the generic hint when no single address exists', () => {
    expect(messageFor(refusal(), t)).toBe('errors.origin_rejected {}')
  })
})
