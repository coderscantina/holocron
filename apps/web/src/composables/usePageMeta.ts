// Adapted from Kit (f447e797, MIT): resources/js/composables/usePageMeta.ts, without breadcrumbs:
// the header shows workspace and site context instead.
import { computed, onScopeDispose, ref, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'

import { runtimeConfig } from '~/services/runtime-config'

/** One page owns the title at a time; a late unmount cannot wipe the incoming page's title. */
const current = ref<string | null>(null)
let owner: symbol | null = null

/** Sets the document title and the text the shell announces after navigation. */
export function usePageMeta(title: MaybeRefOrGetter<string>): void {
  const token = Symbol('page-meta')

  watchEffect(() => {
    current.value = toValue(title)
    owner = token
    document.title = `${current.value} · ${runtimeConfig.instance_name}`
  })

  onScopeDispose(() => {
    if (owner !== token) return
    current.value = null
    owner = null
    document.title = runtimeConfig.instance_name
  })
}

export function useCurrentPageTitle() {
  return computed(() => current.value ?? '')
}
