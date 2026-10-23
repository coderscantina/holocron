import type { Cell, Field, Measure, Metric, Money, Range } from '~/generated'
import { brandName, isChannel, isDeviceClass } from '~/lib/client-looks'
import { fieldLabels, measureLabels, statLabels, weekdayLabels } from '~/lib/exploration-labels'
import { useI18n } from '~/plugins/i18n'

import { useFormat } from './useFormat'

const isWeekday = (value: number): value is keyof typeof weekdayLabels => value >= 1 && value <= 7

/** Labels and number formatting for exploration states and results, in the active locale. */
export function useExplorationFormat() {
  const { t, locale } = useI18n()
  const format = useFormat()

  const fieldLabel = (field: Field): string =>
    typeof field === 'string'
      ? t(fieldLabels[field])
      : t(field.scope === 'event' ? 'explore.eventProperty' : 'explore.sessionProperty', { key: field.key })

  const measureLabel = (measure: Measure): string =>
    typeof measure === 'string'
      ? t(measureLabels[measure])
      : t(statLabels[measure.stat], { key: measure.field.key })

  const regionNames = computed(() => new Intl.DisplayNames(locale.value, { type: 'region', fallback: 'code' }))

  /** A known collection value by name: a browser or system brand, a device class, a channel, a country. */
  const textLabel = (field: Field, value: string): string => {
    if (field === 'device' && isDeviceClass(value)) return t(`explore.values.device.${value}`)
    if (field === 'channel' && isChannel(value)) return t(`explore.values.channel.${value}`)
    if (field === 'country' && /^[A-Z]{2}$/.test(value)) return regionNames.value.of(value) ?? value
    return brandName(field, value) ?? value
  }

  /**
   * A cell as text. Collection's own values for browser, system, device and country read by
   * name; anything else stays data: never translated or rendered as HTML.
   */
  const cellLabel = (field: Field, cell: Cell): string => {
    switch (cell.t) {
      case 'string':
        return textLabel(field, cell.v)
      case 'not_set':
        return t('explore.table.notSet')
      case 'null':
        return t('explore.table.null')
      case 'integer':
        if (field === 'weekday' && isWeekday(cell.v)) return t(weekdayLabels[cell.v])
        if (field === 'hour') return `${String(cell.v).padStart(2, '0')}:00`
        return format.integer(cell.v)
      case 'bool':
        return t(cell.v ? 'explore.filters.true' : 'explore.filters.false')
      default:
        return String(cell.v)
    }
  }

  /** An exact amount in its currency. The decimal string is formatted as is, never through a float. */
  const money = (value: Money): string =>
    new Intl.NumberFormat(locale.value, {
      style: 'currency',
      currency: value.currency,
      minimumFractionDigits: value.exponent,
      maximumFractionDigits: value.exponent,
    }).format(value.amount as Intl.StringNumericLiteral)

  /** A metric in full precision for tables; `compact` shortens counts for summary cards. */
  const metric = (value: Metric | undefined, compact = false): string => {
    if (!value) return ''
    switch (value.t) {
      case 'count':
        return compact ? format.compact(value.v) : format.integer(value.v)
      case 'ratio':
        return new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 1 }).format(value.v)
      case 'millis':
        return format.duration(value.v / 1000)
      case 'decimal':
        return new Intl.NumberFormat(locale.value, { maximumFractionDigits: 4 }).format(Number(value.v))
      case 'money':
        return value.v.length === 0 ? '–' : value.v.map(money).join(' · ')
      case 'unavailable':
        return '–'
    }
  }

  /** Relative change as a signed percentage. */
  const change = (ratio: number) =>
    new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 1, signDisplay: 'exceptZero' }).format(
      ratio
    )

  const date = (ms: number, timezone: string, withTime = false) =>
    new Intl.DateTimeFormat(locale.value, {
      timeZone: timezone,
      dateStyle: 'medium',
      ...(withTime ? { timeStyle: 'short' } : {}),
    }).format(ms)

  const rangeLabel = (range: Range): string => {
    if (range.kind === 'all') return t('explore.range.all')
    if (range.kind === 'days') return t('explore.range.days', { start: range.start, end: range.end })
    if (range.unit === 'day' && range.count === 1) return t('explore.range.today')
    if (range.unit === 'month' && range.count === 12) return t('explore.range.last12Months')
    if (range.unit === 'month' && range.count === 36) return t('explore.range.last3Years')
    if (range.unit === 'month' && range.count === 60) return t('explore.range.last5Years')
    const key =
      range.unit === 'day'
        ? 'explore.range.relativeDays'
        : range.unit === 'week'
          ? 'explore.range.relativeWeeks'
          : 'explore.range.relativeMonths'
    return t(key, { count: range.count })
  }

  return { fieldLabel, measureLabel, textLabel, cellLabel, metric, money, change, date, rangeLabel }
}
