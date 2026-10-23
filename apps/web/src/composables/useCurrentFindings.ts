import { createSharedComposable } from '@vueuse/core'

import { useSiteContext } from '~/composables/useSiteContext'
import { useSiteFindings } from '~/composables/useSiteFindings'
import { FINDINGS_MAX } from '~/generated/limits'

/**
 * The current site's findings for the sidebar badges and the palette. One query and one
 * stream serve every consumer; nothing is read while no site is in view.
 */
export const useCurrentFindings = createSharedComposable(() => {
  const context = useSiteContext()
  return useSiteFindings(() => context.site.value?.id ?? null, FINDINGS_MAX)
})
