import { beforeEach, describe, expect, it } from 'vitest'

import type { ForecastInputs, ReadinessProblem } from '~/generated'
import { composer } from '~/plugins/i18n'

import { blocksCollection, problemDescriptions, problemTitles, useOperatorStatusText } from './operator-status'

/** Every problem `/health/ready` names (`maintenance/health.rs`). */
const problems: ReadinessProblem[] = [
  'disk_full',
  'storage_unavailable',
  'shutting_down',
  'transfer_failing',
  'analytics_stale',
  'recorder_budget',
]

const inputs: ForecastInputs = {
  sampled_at: 1_792_054_800_000,
  retained_records: 100_000_000,
  analytics_bytes: 10_000_000_000,
  bytes_per_record: 100,
  ingest_per_day: 300_000,
  oldest_day: '2025-11-20',
  full_retention_on: '2026-11-20',
  days_until_full_retention: 36,
  sqlite_bytes: 100_000_000,
  replay_cap_bytes: 26_843_545_600,
  disk_reserve_bytes: 4_294_967_296,
  steady_state_bytes: 42_188_512_896,
  used_bytes: 15_100_000_000,
  capacity_bytes: 75_100_000_000,
}

describe('operator status text', () => {
  beforeEach(() => {
    composer.locale.value = 'en'
  })

  it('names every readiness problem in words in both locales', () => {
    const text = useOperatorStatusText()
    expect(text.state([])).toBe('Healthy')
    for (const locale of ['en', 'de'] as const) {
      composer.locale.value = locale
      const titles = problems.map((problem) => text.state([problem]))
      expect(new Set(titles).size, locale).toBe(problems.length)
      for (const problem of problems) {
        expect(composer.te(problemTitles[problem]), `${locale} ${problem}`).toBe(true)
        expect(composer.te(problemDescriptions[problem]), `${locale} ${problem}`).toBe(true)
        expect(text.state([problem]), `${locale} ${problem}`).not.toContain('_')
      }
    }
    composer.locale.value = 'en'
    expect(text.state(['analytics_stale', 'disk_full'])).toBe('Analytics behind')
    expect(problems.filter(blocksCollection)).toEqual(['disk_full', 'storage_unavailable', 'shutting_down'])
  })

  it('says each forecast outcome in one sentence', () => {
    const text = useOperatorStatusText()
    expect(text.forecast({ inputs, outcome: { kind: 'levels_off', size_bytes: 42_188_512_896, on: '2026-11-20' } })).toBe(
      'Disk use levels off near 39.3 GB on Nov 20, 2026.'
    )
    expect(text.forecast({ inputs, outcome: { kind: 'reaches_reserve', on: '2026-11-09' } })).toBe(
      'Free space reaches the disk reserve on Nov 9, 2026.'
    )
    expect(text.forecast({ inputs, outcome: { kind: 'not_enough_history', days: 10 } })).toBe(
      'Not enough history for a forecast: 10 of 14 days retained.'
    )
    expect(text.forecast(null)).toContain('No storage sample yet')
  })
})
