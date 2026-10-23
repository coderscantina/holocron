import { computed, ref, toValue, watch, type MaybeRefOrGetter } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import type { ExplorationState } from '~/generated'
import { decodeState, encodeState, fitsUrl, normalize } from '~/services/exploration'

export type StateProblem = 'unreadable' | 'version' | 'oversize' | 'site'

/**
 * The route adapter of the exploration service: the state lives in the `s` query parameter,
 * so a URL, Back and Share carry it. A deliberate edit pushes one history entry; only
 * canonicalization replaces. Without `s` the page's default applies.
 */
export function useExploration(site: MaybeRefOrGetter<string>, defaults: () => ExplorationState | null) {
  const route = useRoute()
  const router = useRouter()

  const raw = computed(() => (typeof route.query.s === 'string' ? route.query.s : null))
  const decoded = computed(() => (raw.value === null ? null : decodeState(raw.value)))
  /** Set when an edit would make the URL too long; the edit is not applied. */
  const oversize = ref(false)

  const problem = computed<StateProblem | null>(() => {
    const result = decoded.value
    if (result && !result.ok) return result.reason
    if (result?.ok && result.state.site !== toValue(site)) return 'site'
    return null
  })

  const state = computed<ExplorationState | null>(() => {
    if (problem.value) return null
    const result = decoded.value
    return result?.ok ? result.state : defaults()
  })

  const write = (next: ExplorationState | null, mode: 'push' | 'replace') => {
    const query = { ...route.query }
    if (next) query.s = encodeState(next)
    else delete query.s
    return mode === 'push' ? router.push({ query }) : router.replace({ query })
  }

  /** Applies one deliberate step: one history entry, so Back undoes exactly it. */
  const commit = (next: ExplorationState) => {
    const normalized = normalize(next)
    if (!fitsUrl(normalized)) {
      oversize.value = true
      return
    }
    oversize.value = false
    const current = state.value
    if (current && encodeState(current) === encodeState(normalized) && raw.value !== null) return
    return write(normalized, 'push')
  }

  /** Drops the state from the URL, back to the page's default. A separate action from clearing filters. */
  const reset = () => {
    oversize.value = false
    return write(null, 'push')
  }

  // A readable but non-canonical URL is rewritten in place, so equal states share one address.
  watch(
    [raw, decoded],
    ([text, result]) => {
      if (text !== null && result?.ok && encodeState(result.state) !== text) void write(result.state, 'replace')
    },
    { immediate: true }
  )

  return { state, problem, oversize, commit, reset }
}
