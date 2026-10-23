import type { SitePace } from '~/lib/site-pace'
import { signedPercent } from '~/lib/site-pace'
import { useI18n } from '~/plugins/i18n'

/** A site's pace in words and its tone, as the workspace status list and tiles show it. */
export function usePaceWords() {
  const { t } = useI18n()
  return {
    text: (pace: SitePace) => {
      switch (pace.kind) {
        case 'new':
          return t('context.pace.new')
        case 'quiet':
          return t('context.pace.quiet', { count: pace.views })
        case 'usual':
          return t('context.pace.usual')
        default:
          return t('context.pace.change', { change: signedPercent(pace.ratio) })
      }
    },
    tone: (pace: SitePace | null) =>
      pace?.kind === 'above' ? 'text-success' : pace?.kind === 'below' ? 'text-destructive' : 'text-muted',
  }
}
