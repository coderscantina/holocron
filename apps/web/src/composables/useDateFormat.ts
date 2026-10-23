import { useI18n } from '~/plugins/i18n'

const cache = new Map<string, Intl.DateTimeFormat>()

/** Dates of account records (keys, invitations, shares) in the reader's locale and timezone. */
export function useDateFormat() {
  const { locale } = useI18n()
  const format = (options: Intl.DateTimeFormatOptions) => {
    const key = `${locale.value}:${JSON.stringify(options)}`
    let formatter = cache.get(key)
    if (!formatter) {
      formatter = new Intl.DateTimeFormat(locale.value, options)
      cache.set(key, formatter)
    }
    return formatter
  }
  return {
    /** Unix milliseconds as a date, "5 Oct 2026". */
    date: (ms: number) => format({ dateStyle: 'medium' }).format(ms),
    /** Unix milliseconds with the time, for tooltips. */
    dateTime: (ms: number) => format({ dateStyle: 'medium', timeStyle: 'short' }).format(ms),
  }
}
