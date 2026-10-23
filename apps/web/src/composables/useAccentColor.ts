// Adapted from Kit (MIT): resources/js/composables/useAccentColor.ts.
import { createSharedComposable } from '@vueuse/core'
import { ref, watchEffect } from 'vue'

import type { Accent } from '~/generated'

/** The presets in `assets/css/app.css`, default first. */
export const accentColors = ['indigo', 'blue', 'violet', 'rose', 'orange', 'amber', 'emerald', 'cyan'] as const satisfies readonly Accent[]

export type AccentColor = (typeof accentColors)[number]

/** Shared with the inline script in index.html, which applies the attribute before first paint. */
const STORAGE_KEY = 'holocron:accent'

const isAccentColor = (value: unknown): value is AccentColor => accentColors.some((accent) => accent === value)

const readStoredAccent = (): AccentColor => {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    return isAccentColor(stored) ? stored : 'indigo'
  } catch {
    return 'indigo'
  }
}

const useAccentColorBase = () => {
  const accent = ref<AccentColor>(readStoredAccent())

  watchEffect(() => {
    document.documentElement.dataset.accent = accent.value
  })

  const setAccent = (next: AccentColor): void => {
    accent.value = next
    try {
      window.localStorage.setItem(STORAGE_KEY, next)
    } catch {
      // The accent still applies for this session.
    }
  }

  return { accent, setAccent, accentColors }
}

/** One source of truth: the picker and anything else reading it stay in step. */
export const useAccentColor = createSharedComposable(useAccentColorBase)
