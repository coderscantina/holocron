// Adapted from Kit (f447e797, MIT): resources/js/composables/useColorMode.ts.
import { createSharedComposable } from '@vueuse/core'
import { computed, ref, watchEffect } from 'vue'

export const colorModes = ['light', 'dark', 'system'] as const

export type ColorMode = (typeof colorModes)[number]

/** Shared with the inline script in index.html, which applies the class before first paint. */
const STORAGE_KEY = 'holocron:color-mode'

const isColorMode = (value: unknown): value is ColorMode => colorModes.some((mode) => mode === value)

const readStoredMode = (): ColorMode => {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    return isColorMode(stored) ? stored : 'system'
  } catch {
    return 'system'
  }
}

const useColorModeBase = () => {
  const mode = ref<ColorMode>(readStoredMode())
  const media = window.matchMedia('(prefers-color-scheme: dark)')
  const prefersDark = ref(media.matches)
  media.addEventListener('change', (event) => {
    prefersDark.value = event.matches
  })

  const resolved = computed<Exclude<ColorMode, 'system'>>(() =>
    mode.value === 'system' ? (prefersDark.value ? 'dark' : 'light') : mode.value
  )
  const isDark = computed(() => resolved.value === 'dark')

  watchEffect(() => {
    const root = document.documentElement
    root.classList.toggle('dark', isDark.value)
    // Native widgets (scrollbars, form controls) read this, not the class.
    root.style.colorScheme = resolved.value
  })

  const setMode = (next: ColorMode): void => {
    mode.value = next
    try {
      window.localStorage.setItem(STORAGE_KEY, next)
    } catch {
      // The mode still applies for this session.
    }
  }

  return { mode, resolved, isDark, setMode, colorModes }
}

/** One source of truth: several toggles on screen stay in step. */
export const useColorMode = createSharedComposable(useColorModeBase)
