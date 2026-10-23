import { useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'

/** The caller's second factor: enrolled, required, recovery codes left. */
export function useTwoFactorStatus() {
  return useQuery({
    queryKey: queryKeys.twoFactor,
    queryFn: ({ signal }) => accountResource.twoFactor(signal),
  })
}
