/**
 * The replay frame. Served by the Rust server under a policy without network access and a
 * sandbox without navigation, popups or forms (decision 0011). It renders captured documents
 * with rrweb's Replayer into an iframe that runs no scripts and takes no pointer input, and
 * talks to the dashboard only through `postMessage`. It never fetches anything.
 */
import 'rrweb/dist/style.css'
import './frame.css'

import { Replayer, type eventWithTime } from 'rrweb'

import { parseToFrame, type FromFrame } from '~/lib/replay-frame'

const stage = document.body.appendChild(document.createElement('div'))
stage.id = 'stage'
const TICK_MS = 250

let replayer: Replayer | null = null
let playing = false
let size = { width: 0, height: 0 }
function send(message: FromFrame) {
  window.parent.postMessage(message, location.origin)
}

/** rrweb events as the API returns them; anything else is dropped before the Replayer sees it. */
function isEvent(value: unknown): value is eventWithTime {
  return (
    typeof value === 'object' &&
    value !== null &&
    'type' in value &&
    typeof value.type === 'number' &&
    'timestamp' in value &&
    typeof value.timestamp === 'number' &&
    Number.isFinite(value.timestamp)
  )
}

function isSize(value: unknown): value is { width: number; height: number } {
  return (
    typeof value === 'object' &&
    value !== null &&
    'width' in value &&
    typeof value.width === 'number' &&
    'height' in value &&
    typeof value.height === 'number'
  )
}

function parseEvents(json: string): eventWithTime[] {
  try {
    const parsed: unknown = JSON.parse(json)
    return Array.isArray(parsed) ? parsed.filter(isEvent) : []
  } catch {
    return []
  }
}

function fit() {
  if (!replayer || size.width === 0 || size.height === 0) return
  const scale = Math.min(stage.clientWidth / size.width, stage.clientHeight / size.height)
  const left = (stage.clientWidth - size.width * scale) / 2
  replayer.wrapper.style.transform = `translate(${left}px, 0) scale(${scale})`
}

function report() {
  if (!replayer) return
  const { startTime, totalTime } = replayer.getMetaData()
  send({ type: 'loaded', start: startTime, duration: totalTime, ...size })
}

function load(events: eventWithTime[]) {
  replayer?.destroy()
  replayer = null
  playing = false
  if (events.length < 2 || !events.some((event) => event.type === 2)) {
    send({ type: 'failed' })
    return
  }
  replayer = new Replayer(events, {
    root: stage,
    showWarning: false,
    mouseTail: false,
    UNSAFE_replayCanvas: false,
    useVirtualDom: false,
  })
  replayer.on('resize', (dimension) => {
    if (!isSize(dimension)) return
    size = { width: dimension.width, height: dimension.height }
    fit()
    report()
  })
  replayer.on('finish', () => {
    playing = false
    tick()
  })
  replayer.pause(0)
  report()
}

function tick() {
  if (replayer) send({ type: 'time', at: replayer.getCurrentTime(), playing })
}

window.addEventListener('message', (event) => {
  if (event.source !== window.parent || event.origin !== location.origin) return
  const message = parseToFrame(event.data)
  if (!message) return
  switch (message.type) {
    case 'load':
      return load(parseEvents(message.events))
    case 'append':
      parseEvents(message.events).forEach((replayEvent) => replayer?.addEvent(replayEvent))
      return report()
    case 'play':
      playing = true
      replayer?.play(replayer.getCurrentTime())
      return tick()
    case 'pause':
      playing = false
      replayer?.pause()
      return tick()
    case 'seek':
      if (playing) replayer?.play(message.at)
      else replayer?.pause(message.at)
      return tick()
    case 'speed':
      return replayer?.setConfig({ speed: message.speed })
    case 'skip':
      return replayer?.setConfig({ skipInactive: message.skip })
  }
})

window.addEventListener('resize', fit)
setInterval(() => {
  if (playing) tick()
}, TICK_MS)
send({ type: 'ready' })
