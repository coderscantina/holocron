import { computed, ref, watch, type WatchSource } from 'vue'

import { ApiError } from '~/api'

/**
 * State a settings form shares with the others. One idempotency key per intent: a retry of the
 * same values replays the stored answer instead of applying twice; any edit starts a new intent.
 */
export function useCommandForm(sources: WatchSource[], error: () => Error | null) {
  const intentKey = ref(crypto.randomUUID())
  const saved = ref(false)
  watch(sources, () => {
    intentKey.value = crypto.randomUUID()
    saved.value = false
  })

  const fieldErrors = computed(() => {
    const current = error()
    return current instanceof ApiError && current.code === 'invalid_arguments'
  })

  const submit = async (run: (idempotencyKey: string) => Promise<unknown>) => {
    saved.value = false
    await run(intentKey.value)
    saved.value = true
  }

  return { saved, fieldErrors, submit }
}
