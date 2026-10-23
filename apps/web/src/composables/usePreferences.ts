import { useMutation, useQuery } from '@tanstack/vue-query'
import { watch } from 'vue'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import type { Preferences } from '~/generated'
import { isSupportedLocale, setLocale } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

import { useAccentColor } from './useAccentColor'
import { useColorMode } from './useColorMode'

/** The stored fields only, without the snapshot's revision and cursor. */
const fields = ({ timezone, default_range, locale, theme, accent }: Preferences): Preferences => ({
  timezone,
  default_range,
  locale,
  theme,
  accent,
})

/** The person's stored display settings, and a save that changes some of them. */
export function usePreferences() {
  const query = useQuery({ queryKey: queryKeys.preferences, queryFn: ({ signal }) => accountResource.preferences(signal) })
  const mutation = useMutation({
    mutationFn: (next: Preferences) => accountResource.updatePreferences(next),
    onSuccess: (saved) => queryClient.setQueryData(queryKeys.preferences, saved),
  })
  const save = (patch: Preferences) => mutation.mutateAsync({ ...fields(query.data.value ?? {}), ...patch })
  return { query, mutation, save }
}

/**
 * Applies the stored language, theme and accent once per sign-in, so another browser picks them
 * up. Later choices in the user menu are saved back, so the two stay in step.
 */
export function useApplyPreferences() {
  const { query } = usePreferences()
  const { setMode } = useColorMode()
  const { setAccent } = useAccentColor()
  let applied = false
  watch(
    () => query.data.value,
    (preferences) => {
      if (!preferences || applied) return
      applied = true
      if (preferences.locale && isSupportedLocale(preferences.locale)) setLocale(preferences.locale)
      if (preferences.theme) setMode(preferences.theme)
      if (preferences.accent) setAccent(preferences.accent)
    },
    { immediate: true }
  )
}
