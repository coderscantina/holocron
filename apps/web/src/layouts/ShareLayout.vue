<script setup lang="ts">
import AppBrand from '~/components/app/AppBrand.vue'
import SkipLink from '~/components/app/SkipLink.vue'
import DbipAttribution from '~/components/DbipAttribution.vue'
import { useColorMode } from '~/composables/useColorMode'
import { runtimeConfig } from '~/services/runtime-config'

// No user menu here, so this is what keeps the page in step with the system scheme.
useColorMode()
</script>

<template>
  <div class="flex min-h-svh flex-col bg-surface">
    <SkipLink />
    <header
      data-frame
      class="flex h-shell-header items-center border-b border-border bg-sidebar px-shell-gutter-x"
    >
      <AppBrand />
    </header>
    <main
      id="main-content"
      tabindex="-1"
      class="mx-auto w-full max-w-7xl min-w-0 flex-1 px-shell-gutter-x py-shell-gutter focus:outline-none"
    >
      <slot />
    </main>
    <footer
      v-if="runtimeConfig.dbip_attribution"
      class="px-shell-gutter-x pb-6 text-center text-xs text-muted"
    >
      <DbipAttribution />
    </footer>
  </div>
</template>
