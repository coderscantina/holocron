// Adapted from Kit (f447e797, MIT): resources/js/lib/app-config.ts. Brand name and version come
// from the runtime config instead, so they change without a rebuild.

export interface AppConfig {
  sidebar: {
    collapsible: boolean
    defaultCollapsed: boolean
    resizable: boolean
    /** Expanded width bounds and default, in pixels. */
    width: { default: number; min: number; max: number }
  }
  /** Nav items the phone tab bar shows before its trailing Menu tab. */
  tabBarItems: number
  /** Row heights and shell padding. `compact` trims roughly 20%. */
  density: 'comfortable' | 'compact'
}

export const appConfig: AppConfig = {
  sidebar: {
    collapsible: true,
    defaultCollapsed: false,
    resizable: true,
    width: { default: 248, min: 208, max: 360 },
  },
  tabBarItems: 3,
  density: 'compact',
}
