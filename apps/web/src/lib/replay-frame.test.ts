import { describe, expect, it } from 'vitest'

import { parseFromFrame, parseToFrame } from './replay-frame'

describe('replay frame messages', () => {
  it('pass only well-formed commands into the frame', () => {
    expect(parseToFrame({ type: 'load', events: '[]' })).toEqual({ type: 'load', events: '[]' })
    expect(parseToFrame({ type: 'seek', at: 1500 })).toEqual({ type: 'seek', at: 1500 })
    expect(parseToFrame({ type: 'load', events: [] })).toBeNull()
    expect(parseToFrame({ type: 'seek', at: -1 })).toBeNull()
    expect(parseToFrame({ type: 'speed', speed: 3 })).toBeNull()
    expect(parseToFrame({ type: 'eval', code: 'alert(1)' })).toBeNull()
    expect(parseToFrame('play')).toBeNull()
  })

  it('pass only well-formed reports out of the frame', () => {
    expect(parseFromFrame({ type: 'time', at: 10, playing: true })).toEqual({ type: 'time', at: 10, playing: true })
    expect(parseFromFrame({ type: 'loaded', start: 1, duration: 2, width: 1280, height: 720 })).toEqual({
      type: 'loaded',
      start: 1,
      duration: 2,
      width: 1280,
      height: 720,
    })
    expect(parseFromFrame({ type: 'time', at: Number.NaN, playing: true })).toBeNull()
    expect(parseFromFrame({ type: 'loaded', duration: 2 })).toBeNull()
    expect(parseFromFrame({ type: 'navigate', url: 'https://example.test' })).toBeNull()
  })
})
