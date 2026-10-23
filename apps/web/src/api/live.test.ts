import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { LiveFrame } from '~/generated'

import { ApiClient } from './client'
import { closeAllLive, joinLive, type LiveListener } from './live'

const subscribed: LiveFrame = { kind: 'subscribed', head: 'analytics:s1@1', cursor: 'analytics:s1@1' }

/** A client whose streams the test drives: `emit` sends a frame, `fail` ends the latest stream. */
function fakeStreams() {
  const client = new ApiClient(() => '/api')
  const closes: Array<ReturnType<typeof vi.fn>> = []
  const sinks: Array<{ data: (text: string) => void; end: () => void }> = []
  const subscribe = vi.spyOn(client, 'subscribe').mockImplementation((_params, data, end) => {
    const close = vi.fn()
    closes.push(close)
    sinks.push({ data, end })
    return close
  })
  return {
    client,
    subscribe,
    closes,
    emit: (frame: LiveFrame) => sinks.at(-1)?.data(JSON.stringify(frame)),
    fail: () => sinks.at(-1)?.end(),
  }
}

function listener() {
  const frames: LiveFrame[] = []
  const dropped = vi.fn()
  const member: LiveListener = { frame: (frame) => frames.push(frame), dropped }
  return { member, frames, dropped }
}

describe('shared live streams', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => {
    closeAllLive()
    vi.useRealTimers()
  })

  it('opens one stream per topic, fans events out and closes after the last leaves', () => {
    const { client, subscribe, closes, emit } = fakeStreams()
    const [a, b, c] = [listener(), listener(), listener()]
    const leaves = [a, b, c].map((one) => joinLive('analytics:s1', 'analytics:s1@1', one.member, client))

    expect(subscribe).toHaveBeenCalledTimes(1)
    emit(subscribed)
    emit({ kind: 'events', cursor: 'analytics:s1@2', events: [] })
    for (const one of [a, b, c]) expect(one.frames.map((frame) => frame.kind)).toEqual(['subscribed', 'events'])

    leaves[0]?.()
    leaves[1]?.()
    expect(closes[0]).not.toHaveBeenCalled()
    leaves[2]?.()
    expect(closes[0]).toHaveBeenCalledTimes(1)
  })

  it('lets a late listener see the stream is up, and keeps other topics apart', () => {
    const { client, subscribe, emit } = fakeStreams()
    joinLive('analytics:s1', 'analytics:s1@1', listener().member, client)
    emit(subscribed)
    const late = listener()
    joinLive('analytics:s1', 'analytics:s1@1', late.member, client)
    joinLive('analytics:s2', 'analytics:s2@1', listener().member, client)

    expect(late.frames).toEqual([subscribed])
    expect(subscribe).toHaveBeenCalledTimes(2)
  })

  it('reconnects once and tells every listener', () => {
    const { client, subscribe, fail } = fakeStreams()
    const [a, b] = [listener(), listener()]
    joinLive('analytics:s1', 'analytics:s1@1', a.member, client)
    joinLive('analytics:s1', 'analytics:s1@1', b.member, client)

    fail()
    expect(a.dropped).toHaveBeenCalledTimes(1)
    expect(b.dropped).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(1000)
    expect(subscribe).toHaveBeenCalledTimes(2)
  })
})
