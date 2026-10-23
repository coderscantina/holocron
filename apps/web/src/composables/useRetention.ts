import { useMutation, useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { retentionResource } from '~/api/resources/retention'
import { sitesResource } from '~/api/resources/sites'
import type { RetentionPreview, SitesUpdateRetentionArgs } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

import { useConfirm } from './useConfirm'

export const useRetentionSettings = () =>
  useQuery({ queryKey: queryKeys.retentionSettings, queryFn: ({ signal }) => retentionResource.settings(signal) })

export function useUpdateSiteRetention() {
  return useMutation({
    mutationFn: (args: SitesUpdateRetentionArgs) => sitesResource.updateRetention(args),
    onSuccess: (answer) => queryClient.invalidateQueries({ queryKey: queryKeys.site(answer.site.id) }),
  })
}

export function useUpdateDefaultRetention() {
  return useMutation({
    mutationFn: retentionResource.updateDefault,
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.retentionSettings, settings)
      // Sites that follow the default changed their effective retention too.
      return queryClient.invalidateQueries({
        predicate: ({ queryKey }) => queryKey[0] === 'sites.get' || queryKey[0] === 'sites.list',
      })
    },
  })
}

/** Asks before a shortening deletes events, naming how many and from when. */
export function useConfirmRemoval() {
  const { t, locale } = useI18n()
  const { confirm } = useConfirm()

  return (preview: RetentionPreview, timezone?: string) => {
    const events = new Intl.NumberFormat(locale.value).format(preview.events_removed)
    const message =
      preview.cutoff === undefined
        ? t('dataRetention.confirm.noCutoff', { events })
        : t('dataRetention.confirm.message', {
            events,
            date: new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeZone: timezone }).format(preview.cutoff),
          })
    return confirm({
      title: t('dataRetention.confirm.title'),
      message,
      confirmLabel: t('dataRetention.confirm.action'),
      variant: 'destructive',
    })
  }
}
