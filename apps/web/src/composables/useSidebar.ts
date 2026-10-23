// Adapted from Kit (f447e797, MIT): resources/js/composables/useSidebar.ts.
import { createSharedComposable } from '@vueuse/core'
import { computed, ref, watch } from 'vue'

import { appConfig } from '~/lib/app-config'

const COLLAPSED_KEY = 'holocron:sidebar-collapsed'
const WIDTH_KEY = 'holocron:sidebar-width'

/** Rail width matches the header's icon button column, so the two line up. */
const RAIL_WIDTH = 56

const clampWidth = (value: number): number => {
  const { min, max } = appConfig.sidebar.width
  return Math.min(max, Math.max(min, Math.round(value)))
}

const read = (key: string): string | null => {
  try {
    return window.localStorage.getItem(key)
  } catch {
    // Private-mode Safari throws on access rather than returning null.
    return null
  }
}

const write = (key: string, value: string): void => {
  try {
    window.localStorage.setItem(key, value)
  } catch {
    // Storage is unavailable; the choice still applies for this session.
  }
}

const useSidebarBase = () => {
  const config = appConfig.sidebar

  const stored = read(COLLAPSED_KEY)
  const collapsed = ref(config.collapsible ? (stored === null ? config.defaultCollapsed : stored === '1') : false)

  const storedWidth = Number(read(WIDTH_KEY))
  const width = ref(Number.isFinite(storedWidth) && storedWidth > 0 ? clampWidth(storedWidth) : config.width.default)

  /** The mobile drawer is deliberately not persisted: it opens closed, always. */
  const mobileOpen = ref(false)

  watch(collapsed, (value) => write(COLLAPSED_KEY, value ? '1' : '0'))
  watch(width, (value) => write(WIDTH_KEY, String(value)))

  return {
    collapsed,
    width,
    currentWidth: computed(() => (collapsed.value ? RAIL_WIDTH : width.value)),
    mobileOpen,
    collapsible: config.collapsible,
    resizable: config.resizable,
    bounds: config.width,
    toggle: (): void => {
      if (config.collapsible) collapsed.value = !collapsed.value
    },
    setWidth: (value: number): void => {
      width.value = clampWidth(value)
    },
    resetWidth: (): void => {
      width.value = config.width.default
    },
  }
}

/** One source of truth: the header toggle, the rail and `mod+b` stay in step. */
export const useSidebar = createSharedComposable(useSidebarBase)
