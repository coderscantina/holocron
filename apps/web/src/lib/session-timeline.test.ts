import { describe, expect, it } from 'vitest'

import type { SessionEvent, SessionTimeline, TimelineSession } from '~/generated'
import { entryOf, layoutTimeline, longest, timeTicks, timelineDomain, zoneOffset } from '~/lib/session-timeline'

const MINUTE = 60_000
const T0 = Date.UTC(2026, 9, 18, 10)

const session = (id: number, start: number, end: number, events: SessionEvent[] = []): TimelineSession => ({
  session: id,
  person: false,
  start: T0 + start * MINUTE,
  end: T0 + end * MINUTE,
  pageviews: events.filter((event) => event.kind === 'pageview').length,
  custom_events: events.filter((event) => event.kind === 'custom_event').length,
  events,
  truncated: false,
})

const timeline = (sessions: TimelineSession[], total = sessions.length): SessionTimeline => ({
  from: T0,
  to: T0 + 100 * MINUTE,
  total,
  sessions,
})

describe('timelineDomain', () => {
  it('spans the window, or starts near the earliest session when the list is cut', () => {
    const sessions = [session(1, 60, 70), session(2, 80, 90)]
    expect(timelineDomain(timeline(sessions))).toEqual({ start: T0, end: T0 + 100 * MINUTE })
    expect(timelineDomain(timeline(sessions, 50)).start).toBeCloseTo(T0 + 60 * MINUTE - 40 * MINUTE * 0.03)
  })
})

describe('layoutTimeline', () => {
  const domain = { start: T0, end: T0 + 100 * MINUTE }

  it('shares a lane between sessions that do not overlap and opens one for those that do', () => {
    const layout = layoutTimeline([session(1, 0, 10), session(2, 5, 20), session(3, 30, 40)], domain, 1000)
    expect(layout.lanes).toBe(2)
    expect(layout.placed.map((placed) => [placed.session.session, placed.lane])).toEqual([
      [1, 0],
      [2, 1],
      [3, 0],
    ])
  })

  it('gives a one-event session the minimum length and marks custom events', () => {
    const one = session(1, 50, 50, [
      { at: T0 + 50 * MINUTE, kind: 'pageview', path: '/' },
      { at: T0 + 50 * MINUTE, kind: 'custom_event', event_name: 'signup' },
    ])
    const [placed] = layoutTimeline([one], domain, 1000, 12).placed
    expect(placed).toMatchObject({ x: 500, width: 12, marks: [{ x: 0, custom: false }, { x: 0, custom: true }] })
  })

  it('keeps a session that ends at the right edge inside the width', () => {
    const [placed] = layoutTimeline([session(1, 100, 100)], domain, 1000, 10).placed
    expect(placed?.x).toBe(990)
  })
})

describe('timeTicks', () => {
  it('picks the smallest round step that fits and aligns it to local time', () => {
    expect(timeTicks({ start: T0 + 3 * MINUTE, end: T0 + 63 * MINUTE }, 4, 0).map((at) => (at - T0) / MINUTE)).toEqual([15, 30, 45, 60])
    expect(timeTicks({ start: T0, end: T0 + 120 * MINUTE }, 3, 30 * MINUTE).map((at) => (at - T0) / MINUTE)).toEqual([30, 90])
  })

  it('reads a timezone offset', () => {
    expect(zoneOffset(T0, 'Europe/Vienna')).toBe(120 * MINUTE)
    expect(zoneOffset(T0, 'UTC')).toBe(0)
  })
})

describe('rail helpers', () => {
  it('lists the longest sessions and reads entry paths', () => {
    const sessions = [session(1, 0, 5), session(2, 0, 30), session(3, 10, 10), session(4, 40, 45)]
    expect(longest(sessions, 2).map((entry) => entry.session)).toEqual([2, 4])
    expect(entryOf(session(5, 0, 1, [{ at: T0, kind: 'custom_event', path: '/a' }, { at: T0, kind: 'pageview', path: '/b' }]))).toBe('/b')
  })
})
