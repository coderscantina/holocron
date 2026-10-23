import type { RetentionDefaultArgs } from '~/generated'

import { api } from '..'

/** The installation's default event retention (decision 0016). Changing it is for install admins. */
export const retentionResource = {
  settings: (signal?: AbortSignal) => api.query('retention.settings', {}, { signal }),
  /** What the default would remove across the sites, links and pixels that follow it. */
  previewDefault: (args: RetentionDefaultArgs) => api.query('retention.previewDefault', args),
  updateDefault: (args: RetentionDefaultArgs) => api.command('retention.updateDefault', args),
}
