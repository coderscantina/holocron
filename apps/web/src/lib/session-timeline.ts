import type { SessionTimeline, TimelineSession } from '~/generated'

/**
 * Layout of the sessions timeline: each session a line from its first to its last event, packed
 * into lanes so sessions that do not overlap share a row, with a mark per pageview and custom
 * event. Pixels in, pixels out; the component only draws.
 */

export interface Domain {
  start: number
  end: number
}

export interface Mark {
  /** From the line's left end, in pixels. */
  x: number
  custom: boolean
}

export interface PlacedSession {
  session: TimelineSession
  lane: number
  /** Left end and length of the line, in pixels; a one-event session is a dot of the minimum length. */
  x: number
  width: number
  marks: Mark[]
}

export interface Layout {
  placed: PlacedSession[]
  lanes: number
}

/**
 * The time the timeline spans: the whole window, or from shortly before the earliest session
 * shown when the window has more sessions than the timeline carries, so they do not crowd into
 * the right edge of an empty axis.
 */
export function timelineDomain(timeline: SessionTimeline): Domain {
  const earliest = Math.min(...timeline.sessions.map((session) => session.start))
  if (timeline.total <= timeline.sessions.length || !Number.isFinite(earliest)) return { start: timeline.from, end: timeline.to }
  const start = Math.max(timeline.from, earliest - (timeline.to - earliest) * 0.03)
  return { start, end: timeline.to }
}

/**
 * Places sessions in lanes, earliest first: each takes the first lane whose last line ended at
 * least `gap` pixels before it starts. Lines are at least `minWidth` long so a single event
 * stays visible and clickable.
 */
export function layoutTimeline(sessions: TimelineSession[], domain: Domain, width: number, minWidth = 10, gap = 6): Layout {
  const span = Math.max(1, domain.end - domain.start)
  const scale = (at: number) => ((Math.min(Math.max(at, domain.start), domain.end) - domain.start) / span) * width
  const ordered = [...sessions].sort((a, b) => a.start - b.start || a.session - b.session)
  const laneEnds: number[] = []
  const placed = ordered.map((session): PlacedSession => {
    const left = Math.min(scale(session.start), Math.max(0, width - minWidth))
    const right = Math.max(scale(session.end), left + minWidth)
    let lane = laneEnds.findIndex((end) => end + gap <= left)
    if (lane < 0) lane = laneEnds.push(right) - 1
    else laneEnds[lane] = right
    return {
      session,
      lane,
      x: left,
      width: right - left,
      marks: session.events.map((event) => ({
        x: Math.min(Math.max(scale(event.at) - left, 0), right - left),
        custom: event.kind === 'custom_event',
      })),
    }
  })
  return { placed, lanes: laneEnds.length }
}

const MINUTE = 60_000
const STEPS = [5, 10, 15, 30, 60, 120, 180, 360].map((minutes) => minutes * MINUTE)

/**
 * Axis ticks on round local times: the smallest step that leaves at most `count` ticks.
 * `offset` is the timezone's offset from UTC in milliseconds.
 */
export function timeTicks(domain: Domain, count: number, offset: number): number[] {
  const span = domain.end - domain.start
  const step = STEPS.find((candidate) => span / candidate <= count) ?? STEPS.at(-1) ?? MINUTE
  const first = Math.ceil((domain.start + offset) / step) * step - offset
  const ticks: number[] = []
  for (let at = first; at <= domain.end; at += step) ticks.push(at)
  return ticks
}

/** A timezone's offset from UTC at an instant, in milliseconds. */
export function zoneOffset(at: number, timezone: string): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: timezone,
    hourCycle: 'h23',
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
  }).formatToParts(at)
  const part = (type: Intl.DateTimeFormatPartTypes) => Number(parts.find((entry) => entry.type === type)?.value ?? 0)
  const wall = Date.UTC(part('year'), part('month') - 1, part('day'), part('hour'), part('minute'), part('second'))
  return wall - Math.floor(at / 1000) * 1000
}

/** The longest sessions, longest first; ties go to the later one. */
export function longest(sessions: TimelineSession[], count: number): TimelineSession[] {
  return [...sessions]
    .filter((session) => session.end > session.start)
    .sort((a, b) => b.end - b.start - (a.end - a.start) || b.start - a.start)
    .slice(0, count)
}

/** Where a session entered: its first pageview's path, else its first event's. */
export function entryOf(session: TimelineSession): string | null {
  const first = session.events.find((event) => event.kind === 'pageview') ?? session.events[0]
  return first?.path ?? null
}
