/**
 * Messages between the dashboard and the replay frame (`replay-frame.html`). The frame has no
 * network access; the dashboard fetches replay pages through the API and hands their events
 * over as JSON text. Both sides check the sender and every message they receive.
 */

/** Commands the dashboard sends. `events` is the JSON array text of one replay page. */
export type ToFrame =
  | { type: 'load'; events: string }
  | { type: 'append'; events: string }
  | { type: 'play' }
  | { type: 'pause' }
  | { type: 'seek'; at: number }
  | { type: 'speed'; speed: number }
  | { type: 'skip'; skip: boolean }

/**
 * What the frame reports. `start` is the browser time of the first event; other times are
 * milliseconds from it.
 */
export type FromFrame =
  | { type: 'ready' }
  | { type: 'loaded'; start: number; duration: number; width: number; height: number }
  | { type: 'time'; at: number; playing: boolean }
  | { type: 'failed' }

export const SPEEDS = [1, 2, 4, 8] as const

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const isTime = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value) && value >= 0

export function parseToFrame(data: unknown): ToFrame | null {
  if (!isRecord(data)) return null
  switch (data.type) {
    case 'load':
    case 'append':
      return typeof data.events === 'string' ? { type: data.type, events: data.events } : null
    case 'play':
    case 'pause':
      return { type: data.type }
    case 'seek':
      return isTime(data.at) ? { type: 'seek', at: data.at } : null
    case 'speed':
      return typeof data.speed === 'number' && SPEEDS.some((speed) => speed === data.speed)
        ? { type: 'speed', speed: data.speed }
        : null
    case 'skip':
      return typeof data.skip === 'boolean' ? { type: 'skip', skip: data.skip } : null
    default:
      return null
  }
}

export function parseFromFrame(data: unknown): FromFrame | null {
  if (!isRecord(data)) return null
  switch (data.type) {
    case 'ready':
    case 'failed':
      return { type: data.type }
    case 'loaded':
      return isTime(data.start) && isTime(data.duration) && isTime(data.width) && isTime(data.height)
        ? { type: 'loaded', start: data.start, duration: data.duration, width: data.width, height: data.height }
        : null
    case 'time':
      return isTime(data.at) && typeof data.playing === 'boolean'
        ? { type: 'time', at: data.at, playing: data.playing }
        : null
    default:
      return null
  }
}
