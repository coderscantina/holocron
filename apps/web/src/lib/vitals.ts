import type { Metric } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** Web vitals in the order the performance page shows them. */
export const vitals = ['lcp', 'inp', 'cls', 'fcp', 'ttfb'] as const
export type Vital = (typeof vitals)[number]

export const isVital = (value: string): value is Vital => vitals.some((v) => v === value)

export const vitalLabels = {
  lcp: 'performance.vitals.lcp',
  inp: 'performance.vitals.inp',
  cls: 'performance.vitals.cls',
  fcp: 'performance.vitals.fcp',
  ttfb: 'performance.vitals.ttfb',
} as const satisfies Record<Vital, MessageKey>

/**
 * Upper bounds of "good" and "needs improvement" per metric, from web.dev's Core Web Vitals
 * thresholds: milliseconds, and CLS as a plain score.
 */
const thresholds: Record<Vital, [number, number]> = {
  lcp: [2500, 4000],
  inp: [200, 500],
  cls: [0.1, 0.25],
  fcp: [1800, 3000],
  ttfb: [800, 1800],
}

export type Rating = 'good' | 'improve' | 'poor'

/** A percentile as a plain number in its metric's unit, or none when it is unavailable. */
const amount = (metric: Metric | undefined): number | null =>
  metric?.t === 'millis' ? metric.v : metric?.t === 'decimal' ? Number(metric.v) : null

/** The rating of one percentile value, or none when it is unavailable. */
export function rating(vital: Vital, metric: Metric | undefined): Rating | null {
  const value = amount(metric)
  if (value === null) return null
  const [good, improve] = thresholds[vital]
  return value <= good ? 'good' : value <= improve ? 'improve' : 'poor'
}

/**
 * A vital's rating scale as percentages of a track from zero to half again past the "needs
 * improvement" bound, so the poor band always shows: where the good and needs-improvement bands
 * end, and where each value sits. Values past the end sit at 100; unavailable values are null.
 */
export function track(vital: Vital, values: readonly (Metric | undefined)[]) {
  const [good, improve] = thresholds[vital]
  const end = improve * 1.5
  const at = (value: number) => Math.min(100, (Math.max(0, value) / end) * 100)
  return {
    good: at(good),
    improve: at(improve),
    points: values.map((metric) => {
      const value = amount(metric)
      return value === null ? null : at(value)
    }),
  }
}
