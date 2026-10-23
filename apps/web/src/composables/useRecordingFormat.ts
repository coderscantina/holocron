import { useI18n } from '~/plugins/i18n'

const BYTE_UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte'] as const

/** Replay times, playback positions and storage sizes in the active locale. */
export function useRecordingFormat() {
  const { locale } = useI18n()

  return {
    /** Unix milliseconds → "4 Oct 2026, 14:05". */
    dateTime: (ms: number) =>
      new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(ms),
    /** A playback position: "2:05", or "1:02:05" past an hour. */
    clock: (ms: number) => {
      const total = Math.max(0, Math.floor(ms / 1000))
      const [hours, minutes, seconds] = [Math.floor(total / 3600), Math.floor((total % 3600) / 60), total % 60]
      const pad = (value: number) => String(value).padStart(2, '0')
      return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`
    },
    /** 1536 → "1.5 kB", base 1024. */
    bytes: (value: number) => {
      let size = value
      let unit = 0
      while (size >= 1024 && unit < BYTE_UNITS.length - 1) {
        size /= 1024
        unit += 1
      }
      return new Intl.NumberFormat(locale.value, {
        style: 'unit',
        unit: BYTE_UNITS[unit],
        unitDisplay: 'short',
        maximumFractionDigits: 1,
      }).format(size)
    },
  }
}
