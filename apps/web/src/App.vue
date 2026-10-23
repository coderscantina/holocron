<script setup lang="ts">
// The bare Toaster, as in Kit: `assets/css/vue-sonner.css` restyles it onto the design tokens.
import { Toaster } from 'vue-sonner'

import CommandPalette from '~/components/CommandPalette.vue'
import ConfirmDialog from '~/components/ConfirmDialog.vue'
import KeyboardShortcutsDialog from '~/components/KeyboardShortcutsDialog.vue'
import ReauthDialog from '~/components/ReauthDialog.vue'
import AppLayout from '~/layouts/AppLayout.vue'
import GuestLayout from '~/layouts/GuestLayout.vue'
import ShareLayout from '~/layouts/ShareLayout.vue'

const route = useRoute()

/** Nothing renders before the first navigation resolves, so no shell query runs for a share viewer. */
const ready = computed(() => route.matched.length > 0)

const layout = computed(() => {
  if (route.meta.share) return ShareLayout
  return route.meta.guest || route.meta.enroll || route.meta.setup ? GuestLayout : AppLayout
})
const inShell = computed(
  () => ready.value && !route.meta.share && !route.meta.guest && !route.meta.enroll && !route.meta.setup
)

/** Keyed on the matched record, so a page re-enters on a section change, not on a query edit. */
const pageKey = computed(() => `${String(route.matched[0]?.path ?? route.name)}:${String(route.params.siteId ?? '')}`)
</script>

<template>
  <component
    :is="layout"
    v-if="ready"
  >
    <RouterView v-slot="{ Component }">
      <div
        :key="pageKey"
        class="page-enter"
      >
        <component :is="Component" />
      </div>
    </RouterView>
  </component>
  <template v-if="inShell">
    <CommandPalette />
    <KeyboardShortcutsDialog />
    <ConfirmDialog />
    <ReauthDialog />
  </template>
  <Toaster
    position="bottom-right"
    :duration="4000"
    :visible-toasts="3"
    :mobile-offset="{ bottom: 'calc(var(--shell-tabbar-height) + env(safe-area-inset-bottom) + 12px)' }"
    close-button
    rich-colors
  />
</template>
