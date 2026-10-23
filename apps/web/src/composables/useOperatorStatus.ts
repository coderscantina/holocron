import { useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import { useSession } from '~/composables/useSession'

const MINUTE_MS = 60_000

/**
 * `operator.status` for install admins, refreshed every minute and shared by the admin status
 * page and the sidebar server card. Nobody else ever requests it.
 */
export function useOperatorStatus() {
  const { account } = useSession()
  const isInstallAdmin = computed(() => account.value?.install_role === 'install_admin')
  const query = useQuery({
    queryKey: queryKeys.operatorStatus,
    queryFn: ({ signal }) => adminResource.operatorStatus(signal),
    enabled: isInstallAdmin,
    staleTime: MINUTE_MS / 2,
    refetchInterval: MINUTE_MS,
  })
  return { isInstallAdmin, query }
}
