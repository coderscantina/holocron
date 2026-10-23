// From Kit (f447e797, MIT): resources/js/composables/useCommandPalette.ts.
import { ref } from 'vue'

/** Open state of the single palette mounted in App.vue, shared by its trigger and shortcut. */
const isOpen = ref(false)

export function useCommandPalette() {
  return {
    isOpen,
    open: (): void => {
      isOpen.value = true
    },
    close: (): void => {
      isOpen.value = false
    },
    toggle: (): void => {
      isOpen.value = !isOpen.value
    },
  }
}

/** Results shown per group before "Show all". */
export const GROUP_LIMIT = 8

export interface PaletteResult {
  id: string
  label: string
}

/** The results of a group matching the search, capped unless the group is expanded. */
export function visibleResults<T extends PaletteResult>(items: T[], search: string, expanded: boolean) {
  const needle = search.trim().toLowerCase()
  const matches = needle === '' ? items : items.filter((item) => item.label.toLowerCase().includes(needle))
  return expanded ? { shown: matches, total: matches.length } : { shown: matches.slice(0, GROUP_LIMIT), total: matches.length }
}
