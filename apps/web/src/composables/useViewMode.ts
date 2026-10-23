import { useLocalStorage } from '@vueuse/core'
import { computed, type WritableComputedRef } from 'vue'

/**
 * Which of a page's views is shown, remembered in this browser under `holocron:<page>-view`.
 * The first mode is the default; a stored value that is no longer a mode falls back to it.
 */
export function useViewMode<const Mode extends string>(page: string, modes: readonly [Mode, ...Mode[]]): WritableComputedRef<Mode> {
  const [fallback] = modes
  const stored = useLocalStorage<string>(`holocron:${page}-view`, fallback)
  const isMode = (value: string): value is Mode => modes.some((mode) => mode === value)
  return computed({
    get: () => (isMode(stored.value) ? stored.value : fallback),
    set: (mode) => {
      stored.value = mode
    },
  })
}
