import { describe, expect, it, vi } from 'vitest'

import { ApiClient } from '~/api/client'
import { openLive } from '~/api/live'
import { queryKeys } from '~/api/query-keys'
import type { Account } from '~/generated'
import { queryClient } from '~/plugins/vue-query'

import { applyLoginResult, currentSession, forgetEverything } from './useSession'

const account: Account = { id: 'u1', email: 'a@example.test', name: 'A', install_role: 'user' }

/** A client whose streams stay open until closed, recording each close. */
function streamingClient() {
  const closed = vi.fn()
  const client = new ApiClient(() => '/api')
  vi.spyOn(client, 'subscribe').mockImplementation(() => closed)
  return { client, closed }
}

describe('session state', () => {
  it('drops every cached result and closes every live stream when forgetting', () => {
    queryClient.setQueryData(queryKeys.site('s1'), { site: {} })
    queryClient.setQueryData(queryKeys.share('t1'), { name: 'x' })
    const { client, closed } = streamingClient()
    const onError = vi.fn()
    openLive('analytics:s1', 'analytics:s1@1', () => {}, onError, client)
    openLive('analytics:s2', 'analytics:s2@1', () => {}, onError, client)

    forgetEverything()

    expect(queryClient.getQueryCache().getAll()).toHaveLength(0)
    expect(closed).toHaveBeenCalledTimes(2)
    expect(onError).not.toHaveBeenCalled()
  })

  it('keeps the cache through the second-factor step and replaces it once signed in', () => {
    queryClient.setQueryData(queryKeys.workspaces, { workspaces: [] })

    expect(applyLoginResult({ status: 'second_factor_required' })).toBe('second_factor')
    expect(queryClient.getQueryData(queryKeys.workspaces)).toBeDefined()

    expect(applyLoginResult({ status: 'signed_in', account })).toBe('signed_in')
    expect(queryClient.getQueryData(queryKeys.workspaces)).toBeUndefined()
    expect(currentSession()).toEqual({ account, enrollment_only: false })
  })

  it('opens an enrollment-only session when 2FA is required but missing', () => {
    expect(applyLoginResult({ status: 'enrollment_required', account })).toBe('enroll')
    expect(currentSession()).toEqual({ account, enrollment_only: true })
  })
})
