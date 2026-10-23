import type { Cell, Condition, Field, Value } from '~/generated'

/**
 * Icons and display names for the values collection stores for browsers, systems and devices.
 * The keys are what `classify` in the server's `collect/client.rs` and upstream's
 * `detect-browser` produce, so imported history matches too. Brand names stay untranslated;
 * device classes are translated through `explore.values.device`.
 */
interface Look {
  icon: string
  name?: string
}

const BROWSERS: Record<string, Look> = {
  chrome: { icon: 'brand:googlechrome', name: 'Chrome' },
  crios: { icon: 'brand:googlechrome', name: 'Chrome (iOS)' },
  firefox: { icon: 'brand:firefoxbrowser', name: 'Firefox' },
  fxios: { icon: 'brand:firefoxbrowser', name: 'Firefox (iOS)' },
  safari: { icon: 'brand:safari', name: 'Safari' },
  ios: { icon: 'brand:safari', name: 'Safari (iOS)' },
  'ios-webview': { icon: 'brand:safari', name: 'iOS web view' },
  'edge-chromium': { icon: 'brand:microsoftedge', name: 'Edge' },
  edge: { icon: 'brand:microsoftedge', name: 'Edge (legacy)' },
  'edge-ios': { icon: 'brand:microsoftedge', name: 'Edge (iOS)' },
  opera: { icon: 'brand:opera', name: 'Opera' },
  'opera-mini': { icon: 'brand:opera', name: 'Opera Mini' },
  samsung: { icon: 'lucide:globe', name: 'Samsung Internet' },
  yandexbrowser: { icon: 'lucide:globe', name: 'Yandex Browser' },
  facebook: { icon: 'brand:facebook', name: 'Facebook' },
  instagram: { icon: 'brand:instagram', name: 'Instagram' },
  ie: { icon: 'brand:internetexplorer', name: 'Internet Explorer' },
  android: { icon: 'brand:android', name: 'Android browser' },
  'chromium-webview': { icon: 'brand:googlechrome', name: 'Chrome web view' },
}

const SYSTEMS: Record<string, Look> = {
  iOS: { icon: 'brand:apple' },
  'Mac OS': { icon: 'brand:apple', name: 'macOS' },
  'Android OS': { icon: 'brand:android', name: 'Android' },
  'Chrome OS': { icon: 'brand:googlechrome', name: 'ChromeOS' },
  Linux: { icon: 'brand:linux' },
}

const DEVICES: Record<string, string> = {
  desktop: 'lucide:monitor',
  laptop: 'lucide:laptop',
  mobile: 'lucide:smartphone',
  tablet: 'lucide:tablet',
  console: 'lucide:gamepad-2',
  smarttv: 'lucide:tv',
}

export type DeviceClass = 'desktop' | 'laptop' | 'mobile' | 'tablet' | 'console' | 'smarttv'

export const isDeviceClass = (value: string): value is DeviceClass => value in DEVICES

/** The channels the server's classification produces (`channel` in `explore/sql.rs`). */
const CHANNELS = [
  'direct',
  'referral',
  'organicSearch',
  'paidSearch',
  'organicSocial',
  'paidSocial',
  'email',
  'sms',
  'affiliate',
  'llm',
  'paidAds',
  'organicShopping',
  'paidShopping',
  'organicVideo',
  'paidVideo',
] as const

export type Channel = (typeof CHANNELS)[number]

export const isChannel = (value: string): value is Channel => CHANNELS.some((channel) => channel === value)

const systemLook = (value: string): Look | undefined =>
  SYSTEMS[value] ?? (value.startsWith('Windows') ? { icon: 'brand:windows' } : undefined)

/** The icon for a text value of a field, or null when the field has no icons or the value is unknown. */
export function valueIcon(field: Field, value: string): string | null {
  switch (field) {
    case 'browser':
      return BROWSERS[value]?.icon ?? 'lucide:globe'
    case 'os':
      return systemLook(value)?.icon ?? 'lucide:app-window'
    case 'device':
      return DEVICES[value] ?? 'lucide:monitor-smartphone'
    default:
      return null
  }
}

/** The ISO country code a cell or value holds, for its flag; null for any other field or an unset value. */
export const countryCode = (field: Field, cell: Cell | Value): string | null =>
  field === 'country' && cell.t === 'string' ? cell.v : null

/** The one country a filter condition names; substring and range matches name none. */
export const conditionCountry = (condition: Condition): string | null =>
  condition.value && (condition.cmp === 'eq' || condition.cmp === 'neq')
    ? countryCode(condition.field, condition.value)
    : null

/** The brand name of a browser or system value, or null to show the stored value as it is. */
export function brandName(field: Field, value: string): string | null {
  if (field === 'browser') return BROWSERS[value]?.name ?? null
  if (field === 'os') return systemLook(value)?.name ?? null
  return null
}
