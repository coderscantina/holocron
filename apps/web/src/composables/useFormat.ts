import { useI18n } from '~/plugins/i18n'

/** Intl formatters are costly to build and a table renders one per cell; keyed by locale and options. */
const cache = new Map<string, Intl.NumberFormat>()

function numberFormat(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}:${JSON.stringify(options)}`
  let formatter = cache.get(key)
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, options)
    cache.set(key, formatter)
  }
  return formatter
}

/** Locale-aware number formatting bound to the active locale. */
export function useFormat() {
  const { locale } = useI18n()

  return {
    /** 3630 → "3.63K" in English, "3630" in German, where compact starts at a million. */
    compact: (value: number) =>
      numberFormat(locale.value, { notation: 'compact', maximumFractionDigits: 2 }).format(value),
    integer: (value: number) => numberFormat(locale.value, { maximumFractionDigits: 0 }).format(value),
    /** 0.49 → "49%". */
    percent: (ratio: number) => numberFormat(locale.value, { style: 'percent', maximumFractionDigits: 0 }).format(ratio),
    /** 1800 → "1,800 ms", for values that are measured in milliseconds, like web vitals. */
    milliseconds: (value: number) =>
      numberFormat(locale.value, { style: 'unit', unit: 'millisecond', unitDisplay: 'short' }).format(value),
    /** 123 seconds → "2 min, 3 sec" style, in the locale's own unit words. */
    duration: (seconds: number) => {
      const minutes = Math.floor(seconds / 60)
      const rest = Math.round(seconds % 60)
      const unit = (value: number, name: 'minute' | 'second') =>
        numberFormat(locale.value, { style: 'unit', unit: name, unitDisplay: 'narrow' }).format(value)
      return minutes > 0 ? `${unit(minutes, 'minute')} ${unit(rest, 'second')}` : unit(rest, 'second')
    },
  }
}
