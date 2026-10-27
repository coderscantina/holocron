import { describe, expect, test } from 'bun:test'
import { parseOptions } from './compare.ts'
import { classifyDays, compareTop, countVerdict, dayBounds, siteProblems } from './lib.ts'

const hours = (b: { start: number; end: number }) => (b.end - b.start) / 3_600_000

describe('day bounds', () => {
  test('are 23 and 25 hours long across DST changes', () => {
    expect(hours(dayBounds('2026-03-08', 'America/New_York'))).toBe(23)
    expect(hours(dayBounds('2026-11-01', 'America/New_York'))).toBe(25)
    expect(hours(dayBounds('2026-10-25', 'Europe/Berlin'))).toBe(25)
    expect(hours(dayBounds('2026-10-24', 'Europe/Berlin'))).toBe(24)
  })

  test('start at local midnight, including half hour zones', () => {
    expect(new Date(dayBounds('2026-10-25', 'Europe/Berlin').start).toISOString()).toBe('2026-10-24T22:00:00.000Z')
    expect(new Date(dayBounds('2026-10-25', 'Asia/Kolkata').start).toISOString()).toBe('2026-10-24T18:30:00.000Z')
  })
})

describe('tolerances', () => {
  test('pageviews allow 1%, visitors 5%, tiny counts pass', () => {
    expect(countVerdict(1000, 1009, 0.01)).toBe('ok')
    expect(countVerdict(1000, 1020, 0.01)).toBe('differs')
    expect(countVerdict(1000, 960, 0.05)).toBe('ok')
    expect(countVerdict(1000, 900, 0.05)).toBe('differs')
    expect(countVerdict(3, 5, 0.01)).toBe('ok')
  })

  test('a day with no pageviews on a side is an outage, not a difference', () => {
    const u = new Map([['2026-10-23', { pageviews: 100, visitors: 40 }], ['2026-10-24', { pageviews: 90, visitors: 30 }]])
    const h = new Map([['2026-10-23', { pageviews: 100, visitors: 41 }]])
    const { rows, outages } = classifyDays(['2026-10-23', '2026-10-24'], u, h)
    expect(rows.map((r) => r.day)).toEqual(['2026-10-23'])
    expect(outages).toMatchObject([{ day: '2026-10-24', zero: 'holocron' }])
    const top = compareTop(['/'], ['/'])
    expect(siteProblems({ days: rows, outages, pages: top, countries: top })).toEqual(['1 day with traffic on one side only'])
  })
})

describe('top lists', () => {
  const list = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j']

  test('same set in a slightly different order passes', () => {
    expect(compareTop(list, ['b', 'a', 'c', 'd', 'e', 'f', 'g', 'h', 'j', 'i']).verdict).toBe('ok')
  })

  test('low overlap or shuffled ranks fail and name the missing entries', () => {
    const other = compareTop(list, ['a', 'b', 'c', 'x', 'y', 'z', 'q', 'r', 's', 't'])
    expect(other.verdict).toBe('differs')
    expect(other.onlyUmami).toEqual(['d', 'e', 'f', 'g', 'h', 'i', 'j'])
    expect(compareTop(list, [...list].reverse()).verdict).toBe('differs')
  })
})

describe('options', () => {
  const env = { HOLOCRON_API_KEY: 'k' }
  const base = ['--umami', 'mysql://u@h/umami', '--holocron', 'https://h.example', '--from', '2026-10-23']

  test('takes the Umami password from the environment', () => {
    expect(parseOptions(base, { ...env, UMAMI_PASSWORD: 'pw' }).umami.password).toBe('pw')
  })

  test('refuses bad input', () => {
    expect(() => parseOptions(base, {})).toThrow('HOLOCRON_API_KEY')
    expect(() => parseOptions([...base, '--to', '2026-10-01'], env)).toThrow('before')
    expect(() => parseOptions([...base, '--site', 'nope'], env)).toThrow('UUID')
    expect(() => parseOptions(['--umami', 'http://x', ...base.slice(2)], env)).toThrow('--umami')
  })
})
