// Adapted from Kit (f447e797, MIT): resources/js/plugins/vue-query.ts.
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import type { App } from 'vue'

import { ApiError } from '~/api/client'

/** Permission and argument failures surface at once; only transport trouble is retried. */
const PERMANENT = new Set([400, 401, 403, 404, 409, 413, 422, 429])

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Control data changes rarely and every change goes through a command that updates the
      // cache, so focus refetches would only repeat reads.
      refetchOnWindowFocus: false,
      retry: (failureCount, error) => {
        if (error instanceof ApiError && PERMANENT.has(error.status)) return false
        return failureCount < 2
      },
      staleTime: 1000 * 60 * 5,
      gcTime: 1000 * 60 * 30,
    },
  },
})

export function installVueQuery(app: App): void {
  app.use(VueQueryPlugin, { queryClient })
}
