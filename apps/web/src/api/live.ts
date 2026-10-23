import type { LiveFrame } from '~/generated'
import { runtimeConfig } from '~/services/runtime-config'

import type { ApiClient } from './client'

/** Every open stream, so signing out or losing scope can close them all at once. */
const open = new Set<() => void>()

/**
 * Opens `GET /api/subscribe` for a topic from a cursor. Frames arrive as server-sent events;
 * any error closes the stream and reports it, so the caller decides when and where to resume.
 * With a `client` the stream is read through it, carrying its `Authorization` header (share
 * pages); otherwise an `EventSource` sends the session cookie. Returns the close function.
 */
export function openLive(
  topic: string,
  cursor: string,
  onFrame: (frame: LiveFrame) => void,
  onError: () => void,
  client?: ApiClient
): () => void {
  // The server is the type's source: frames are generated from the same registry.
  const parse = (data: string) => onFrame(JSON.parse(data) as LiveFrame)
  let stop: () => void
  const close = () => {
    open.delete(close)
    stop()
  }
  const fail = () => {
    open.delete(close)
    onError()
  }

  if (client) {
    stop = client.subscribe({ topic, cursor }, parse, fail)
  } else {
    const query = new URLSearchParams({ topic, cursor })
    const source = new EventSource(`${runtimeConfig.api_base}/subscribe?${query}`, { withCredentials: true })
    source.onmessage = (event: MessageEvent<string>) => parse(event.data)
    source.onerror = () => {
      source.close()
      fail()
    }
    stop = () => source.close()
  }
  open.add(close)
  return close
}

/** Receives the frames of a shared stream and learns when it drops. */
export type LiveListener = { frame: (frame: LiveFrame) => void; dropped: () => void }

const RETRY_FIRST_MS = 1000
const RETRY_MAX_MS = 30_000

type SharedStream = {
  listeners: Set<LiveListener>
  /** The cursor the stream resumes from, moved forward by every `events` and `resync` frame. */
  position: string
  close: (() => void) | null
  retry: ReturnType<typeof setTimeout> | null
  delay: number
  /** The last `subscribed` frame while the stream is up, replayed to listeners that join later. */
  subscribed: LiveFrame | null
  dropped: boolean
}

/** One stream per topic and client, so a tab holds one connection however many views follow it. */
const streams = new Map<ApiClient | undefined, Map<string, SharedStream>>()

function streamsOf(client: ApiClient | undefined) {
  let topics = streams.get(client)
  if (!topics) streams.set(client, (topics = new Map()))
  return topics
}

/**
 * Follows a topic through a stream shared with every other listener of the same topic and
 * client. The first listener opens it from its cursor, later ones join it at its current
 * position; the last to leave closes it. Unexpected closes retry after 1 s, doubling up to
 * 30 s, and tell every listener. When the server ends the stream (`expired`, `access_revoked`,
 * a `resync` for an unavailable topic) the listeners get that frame and must join again.
 * Returns the leave function.
 */
export function joinLive(topic: string, cursor: string, listener: LiveListener, client?: ApiClient): () => void {
  const topics = streamsOf(client)
  const existing = topics.get(topic)

  const end = (current: SharedStream) => {
    current.close?.()
    if (current.retry) clearTimeout(current.retry)
    Object.assign(current, { close: null, retry: null, subscribed: null })
    if (topics.get(topic) === current) topics.delete(topic)
  }
  const fanOut = (current: SharedStream, call: (member: LiveListener) => void) => {
    for (const member of current.listeners) call(member)
  }

  const connect = (current: SharedStream) => {
    current.retry = null
    current.close = openLive(
      topic,
      current.position,
      (frame) => {
        switch (frame.kind) {
          case 'subscribed':
            current.subscribed = frame
            current.dropped = false
            current.delay = RETRY_FIRST_MS
            break
          case 'events':
            current.position = frame.cursor
            break
          case 'resync':
            current.position = frame.cursor
            current.subscribed = null
            if (frame.reason === 'unavailable') end(current)
            else {
              current.close?.()
              connect(current)
            }
            break
          case 'expired':
          case 'access_revoked':
            end(current)
        }
        fanOut(current, (member) => member.frame(frame))
      },
      () => {
        current.close = null
        current.subscribed = null
        current.dropped = true
        current.retry = setTimeout(() => connect(current), current.delay)
        current.delay = Math.min(current.delay * 2, RETRY_MAX_MS)
        fanOut(current, (member) => member.dropped())
      },
      client
    )
  }

  const joined: SharedStream = existing ?? {
    listeners: new Set(),
    position: cursor,
    close: null,
    retry: null,
    delay: RETRY_FIRST_MS,
    subscribed: null,
    dropped: false,
  }
  joined.listeners.add(listener)
  if (!existing) {
    topics.set(topic, joined)
    connect(joined)
  } else if (joined.subscribed) listener.frame(joined.subscribed)
  else if (joined.dropped) listener.dropped()

  return () => {
    joined.listeners.delete(listener)
    if (joined.listeners.size === 0) end(joined)
  }
}

/** Closes every open stream without reporting errors, so nothing reconnects. */
export function closeAllLive(): void {
  for (const topics of streams.values()) for (const stream of topics.values()) if (stream.retry) clearTimeout(stream.retry)
  streams.clear()
  // Deleting the visited entry while iterating a Set is well defined.
  for (const close of open) close()
}

/**
 * The analytics topic of a site and a cursor on it at a watermark batch, as the server writes
 * them (`apps/server/src/explorations/live.rs`). Share viewers follow it to learn when newer
 * data is visible; an exploration result carries the batch it was read at.
 */
export const analyticsTopic = (site: string) => `analytics:${site}`
export const analyticsCursor = (site: string, batch: number) => `${analyticsTopic(site)}@${batch}`
