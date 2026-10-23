import { useLocalStorage } from '@vueuse/core'
import { onScopeDispose } from 'vue'

import type { RecentKind } from '~/generated'

/** At most this many pings per batch of arrivals, so a burst stays a short run of sounds. */
const MAX_PINGS = 5
const SPACING_S = 0.14

/**
 * A soft ping per arriving event on the realtime globe: one high note for a pageview, two
 * rising notes for a custom event. Off by default; the choice is remembered in this browser.
 * The audio context starts on the first ping after the person turned sound on.
 */
export function useLivePing() {
  const enabled = useLocalStorage('holocron:realtime-sound', false)
  let context: AudioContext | null = null

  const note = (audio: AudioContext, start: number, frequency: number, length: number) => {
    const oscillator = audio.createOscillator()
    const gain = audio.createGain()
    oscillator.type = 'sine'
    oscillator.frequency.setValueAtTime(frequency, start)
    gain.gain.setValueAtTime(0.0001, start)
    gain.gain.exponentialRampToValueAtTime(0.06, start + 0.012)
    gain.gain.exponentialRampToValueAtTime(0.0001, start + length)
    oscillator.connect(gain).connect(audio.destination)
    oscillator.start(start)
    oscillator.stop(start + length + 0.02)
  }

  const play = (kinds: readonly RecentKind[]) => {
    if (!enabled.value || kinds.length === 0) return
    context ??= new AudioContext()
    const audio = context
    void audio.resume()
    kinds.slice(0, MAX_PINGS).forEach((kind, index) => {
      const start = audio.currentTime + 0.02 + index * SPACING_S
      if (kind === 'pageview') note(audio, start, 880, 0.35)
      else {
        note(audio, start, 659, 0.18)
        note(audio, start + 0.09, 988, 0.3)
      }
    })
  }

  onScopeDispose(() => void context?.close())

  return { enabled, play }
}
