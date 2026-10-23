import { useResizeObserver } from '@vueuse/core'
import {
  computed,
  onScopeDispose,
  ref,
  shallowRef,
  watch,
  type Ref,
} from 'vue'

/**
 * The top bar's outlet for a page's scope chips. A page mounts its chips with
 * `<TopBarScope summary="Today · vs typical · 2 filters">…chips…</TopBarScope>`; they render in
 * the bar while the page is mounted and disappear with it. The summary is what the bar shows
 * when the chips do not fit: one chip that opens a popover holding the same chips, so a chip's
 * state and popovers are never duplicated.
 */
const inline = shallowRef<HTMLElement | null>(null)
const overflow = shallowRef<HTMLElement | null>(null)
const summary = ref<string | null>(null)
const collapsed = ref(false)

const target = computed(() => (collapsed.value ? overflow.value : inline.value))

/** Used by `TopBarScope`: where the chips render now, or null while the bar is not mounted. */
export function useTopBarTarget(summaryText: () => string) {
  watch(summaryText, (value) => (summary.value = value), { immediate: true })
  onScopeDispose(() => {
    summary.value = null
    collapsed.value = false
  })
  return target
}

/**
 * Used by the top bar. `area` is the space left for scope chips. The chips collapse when their
 * natural width exceeds it, and expand again once the area is as wide as the chips were.
 */
export function useTopBarScope(area: Ref<HTMLElement | null>) {
  let naturalWidth = 0

  const measure = () => {
    if (!collapsed.value) naturalWidth = inline.value?.offsetWidth ?? 0
    collapsed.value = summary.value !== null && naturalWidth > (area.value?.clientWidth ?? 0)
  }

  useResizeObserver([area, inline], measure)
  watch(summary, measure, { flush: 'post' })

  return {
    summary,
    collapsed,
    setInline: (el: unknown) => (inline.value = el instanceof HTMLElement ? el : null),
    setOverflow: (el: unknown) => (overflow.value = el instanceof HTMLElement ? el : null),
  }
}
