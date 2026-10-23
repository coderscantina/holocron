import { computed, onScopeDispose, ref, shallowRef, toValue, watch, type MaybeRefOrGetter } from 'vue'

import type { ApiClient } from '~/api/client'
import { joinLive } from '~/api/live'
import type { LiveEvent, LiveFrame } from '~/generated'

import { forgetSite, isRefusal } from './useExplore'

export type LiveStatus = 'connecting' | 'current' | 'stale' | 'resyncing' | 'disconnected'

/**
 * One subscription for the life of the calling scope (architecture patterns, Live snapshot
 * adapter). It opens only once a cursor is known, resumes from the last cursor the server
 * handed out, reopens at a `resync` cursor, and on `expired` asks for a fresh snapshot.
 * The stream behind it is shared with every other subscription to the same topic in the tab
 * (`joinLive`), which also owns the retry delay.
 */
export function useSubscription(
  topic: MaybeRefOrGetter<string | null>,
  cursor: MaybeRefOrGetter<string | null>,
  handlers: { events: (events: LiveEvent[]) => void; reset: () => void },
  /** Reads the stream through this client and its credential, as share pages do. */
  client?: ApiClient
) {
  const connected = ref(false)
  const dropped = ref(false)
  const resyncing = ref(false)
  let position: string | null = null
  let leave: (() => void) | null = null

  const stop = () => {
    leave?.()
    leave = null
    connected.value = false
  }

  const open = () => {
    const name = toValue(topic)
    if (!name || !position) return
    // Join before leaving, so moving to a newer cursor keeps a stream this was alone on. A new
    // listener each time, because leaving removes the old one.
    const previous = leave
    leave = joinLive(
      name,
      position,
      {
        frame,
        dropped: () => {
          connected.value = false
          dropped.value = true
        },
      },
      client
    )
    previous?.()
  }

  // The stream is shared: the server ending it (below) leaves nothing to leave, and a plain
  // `resync` is reopened by the stream itself.
  const frame = (message: LiveFrame) => {
    switch (message.kind) {
      case 'subscribed':
        connected.value = true
        dropped.value = false
        resyncing.value = false
        return
      case 'events':
        position = message.cursor
        handlers.events(message.events)
        return
      case 'resync':
        position = message.cursor
        resyncing.value = true
        connected.value = false
        if (message.reason === 'unavailable') {
          // The scope may be gone: a fresh snapshot read either succeeds or refuses and clears.
          leave = null
          handlers.reset()
        }
        return
      case 'expired':
      case 'access_revoked':
        // `access_revoked`: the scope is gone, so the fresh read refuses and clears what it showed.
        leave = null
        connected.value = false
        position = null
        handlers.reset()
    }
  }

  // A new snapshot cursor (first load, reset, other site) restarts the stream from it.
  watch(
    () => [toValue(topic), toValue(cursor)] as const,
    ([name, next]) => {
      if (!name || !next) {
        stop()
        return
      }
      if (next === position && leave) return
      position = next
      open()
    },
    { immediate: true }
  )
  onScopeDispose(stop)

  return { connected, dropped, resyncing, stop }
}

/**
 * A live snapshot: reads it, follows its topic, and reads it again at most once per batch of
 * events. Reads never overlap; events during a read schedule exactly one more. A key change
 * drops the data and starts a new generation, so a late answer for the old key is discarded.
 */
export function useLiveSnapshot<T extends { live_topic: string; live_cursor: string }>(
  key: MaybeRefOrGetter<string | null>,
  load: (key: string, signal: AbortSignal) => Promise<T>
) {
  const data = shallowRef<T | null>(null)
  const error = shallowRef<unknown>(null)
  const loading = ref(false)
  let generation = 0
  let again = false
  let controller: AbortController | null = null

  const read = async () => {
    const current = toValue(key)
    if (!current) return
    if (loading.value) {
      again = true
      return
    }
    const mine = generation
    loading.value = true
    controller = new AbortController()
    try {
      const result = await load(current, controller.signal)
      if (mine !== generation) return
      data.value = result
      error.value = null
    } catch (failure) {
      if (mine !== generation) return
      error.value = failure
      if (isRefusal(failure)) {
        data.value = null
        forgetSite(current)
      }
    } finally {
      if (mine === generation) {
        loading.value = false
        if (again) {
          again = false
          void read()
        }
      }
    }
  }

  watch(
    () => toValue(key),
    () => {
      generation += 1
      controller?.abort()
      loading.value = false
      again = false
      data.value = null
      error.value = null
      void read()
    },
    { immediate: true }
  )

  const subscription = useSubscription(
    () => (error.value && isRefusal(error.value) ? null : (data.value?.live_topic ?? null)),
    () => data.value?.live_cursor ?? null,
    {
      events: () => void read(),
      reset: () => void read(),
    }
  )

  const status = computed<LiveStatus>(() => {
    if (subscription.resyncing.value) return 'resyncing'
    if (subscription.dropped.value) return data.value ? 'stale' : 'disconnected'
    if (!subscription.connected.value) return 'connecting'
    return 'current'
  })

  onScopeDispose(() => controller?.abort())

  return { data, error, loading, status, reload: read }
}
