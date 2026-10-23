<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/layouts/AppLayout.vue.
import AppHeader from '~/components/app/AppHeader.vue'
import AppMobileSidebar from '~/components/app/AppMobileSidebar.vue'
import AppSidebar from '~/components/app/AppSidebar.vue'
import AppTabBar from '~/components/app/AppTabBar.vue'
import SkipLink from '~/components/app/SkipLink.vue'
import ErrorBoundary from '~/components/ErrorBoundary.vue'
import { queryKeys } from '~/api/query-keys'
import { useCurrentPageTitle } from '~/composables/usePageMeta'
import { useApplyPreferences } from '~/composables/usePreferences'
import { useShortcut } from '~/composables/useShortcuts'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const sidebar = useSidebar()
const title = useCurrentPageTitle()
useApplyPreferences()

/** Read by the live region, so a page change is announced, not just painted. */
const announcement = ref('')

/** On a desk the page scrolls inside the canvas, which is the main region, so a new page starts it at the top. */
const panel = ref<HTMLElement | null>(null)

/**
 * A page change moves focus to the main region and announces the new title. Only the path
 * counts: a query-only change (a future exploration edit) keeps focus on the control in use.
 * The layout mounts only after the navigation that brought it in (App.vue waits for the first
 * one), so every navigation seen here is a later one and stealing focus on first render cannot
 * happen.
 */
const removeFocusHook = router.afterEach((to, from) => {
  if (to.path === from.path) return
  void nextTick(() => {
    panel.value?.scrollTo({ top: 0 })
    panel.value?.focus({ preventScroll: true })
    announcement.value = title.value
  })
})
onScopeDispose(removeFocusHook)

/** Switching sites cancels the previous site's requests so a late answer cannot land. */
watch(
  () => route.params.siteId,
  (_, previous) => {
    if (typeof previous === 'string') void queryClient.cancelQueries({ queryKey: queryKeys.site(previous) })
  }
)

useShortcut({
  keys: 'mod+b',
  description: () => t('shortcuts.toggleSidebar'),
  enabled: () => sidebar.collapsible,
  handler: () => sidebar.toggle(),
})
</script>

<template>
  <div class="flex min-h-svh bg-sidebar lg:h-svh lg:overflow-hidden">
    <SkipLink />
    <AppSidebar class="sticky top-0 h-svh" />
    <AppMobileSidebar />

    <div class="flex min-w-0 flex-1 flex-col lg:h-svh">
      <AppHeader />
      <main
        id="main-content"
        ref="panel"
        tabindex="-1"
        class="min-w-0 flex-1 overflow-x-clip bg-surface px-shell-page-x pt-shell-gutter pb-[calc(var(--shell-gutter)+var(--shell-tabbar-height)+env(safe-area-inset-bottom))] focus:outline-none lg:mr-shell-inset lg:mb-shell-inset lg:overflow-y-auto lg:rounded-canvas lg:pb-shell-gutter"
      >
        <ErrorBoundary>
          <slot />
        </ErrorBoundary>
      </main>
    </div>

    <AppTabBar />

    <span
      aria-live="polite"
      aria-atomic="true"
      class="sr-only"
    >
      {{ announcement }}
    </span>
  </div>
</template>
