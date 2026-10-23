// Adapted from Kit (f447e797, MIT): resources/js/plugins/i18n.ts.
import type { App } from 'vue'
import { createI18n, type Composer } from 'vue-i18n'

import de from '~/i18n/de.json'
import en from '~/i18n/en.json'

export type MessageSchema = typeof en

/** Dot-separated paths to the string leaves of a message tree; nested objects are not keys. */
type Paths<T> = {
  [K in keyof T & string]: T[K] extends string ? K : `${K}.${Paths<T[K]>}`
}[keyof T & string]

/** Every key `en.json` defines, e.g. `'shell.userMenu'`. */
export type MessageKey = Paths<MessageSchema>

export const locales = [
  { code: 'en', name: 'English' },
  { code: 'de', name: 'Deutsch' },
] as const

export type LocaleCode = (typeof locales)[number]['code']

const FALLBACK_LOCALE: LocaleCode = 'en'
const LOCALE_STORAGE_KEY = 'holocron:locale'

export const isSupportedLocale = (code: string): code is LocaleCode =>
  locales.some((locale) => locale.code === code)

/** A stored choice first, then the first supported browser language, region ignored. */
function initialLocale(): LocaleCode {
  try {
    const stored = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (stored && isSupportedLocale(stored)) return stored
  } catch {
    // Private-mode Safari throws on access rather than returning null.
  }
  if (typeof navigator === 'undefined') return FALLBACK_LOCALE
  const preferred = navigator.languages?.length ? navigator.languages : [navigator.language]
  for (const tag of preferred) {
    const code = tag?.toLowerCase().split('-')[0]
    if (code && isSupportedLocale(code)) return code
  }
  return FALLBACK_LOCALE
}

export const i18n = createI18n<[MessageSchema], LocaleCode>({
  legacy: false,
  locale: initialLocale(),
  fallbackLocale: FALLBACK_LOCALE,
  messages: { en, de },
})

/** A standalone Composer, so composables and tests translate without an app. */
export const composer = i18n.global as unknown as Composer<{ en: MessageSchema; de: MessageSchema }>

export function setLocale(locale: LocaleCode): void {
  composer.locale.value = locale
  document.documentElement.setAttribute('lang', locale)
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, locale)
  } catch {
    // The choice still applies for this session.
  }
}

export function installI18n(app: App): void {
  app.use(i18n)
  document.documentElement.setAttribute('lang', composer.locale.value)
}

/**
 * A literal key has to exist in the schema. Keys only known as `string` are rejected too:
 * generated enums map to keys through exhaustive records instead of string composition.
 */
function t(key: MessageKey, named?: Record<string, unknown>): string {
  return named ? composer.t(key, named) : composer.t(key)
}

export function useI18n() {
  return {
    t,
    locale: composer.locale,
    locales,
    setLocale,
  }
}
