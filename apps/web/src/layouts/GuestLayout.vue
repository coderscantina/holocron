<script setup lang="ts">
import AppBrand from '~/components/app/AppBrand.vue'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

const { t } = useI18n()

const host = window.location.host

/** The logo's four bars, drawn large beside the column. The third carries the accent. */
const bars = ['h-[46%] bg-fill', 'h-full bg-fill', 'h-[68%] bg-accent/15', 'h-[84%] bg-fill']
</script>

<template>
  <div class="relative flex min-h-svh flex-col overflow-hidden bg-surface">
    <header class="px-6 pt-6 sm:px-12 sm:pt-8 lg:px-24">
      <AppBrand class="-ml-2 w-fit text-base" />
    </header>
    <main
      id="main-content"
      tabindex="-1"
      class="relative z-10 flex flex-1 items-center px-6 py-12 focus:outline-none sm:px-12 lg:px-24"
    >
      <div class="w-full max-w-sm has-[[data-wide]]:max-w-xl">
        <slot />
      </div>
    </main>
    <div
      class="pointer-events-none absolute top-1/2 -right-16 hidden h-[min(70vh,520px)] -translate-y-1/2 items-end gap-8 lg:flex"
      aria-hidden="true"
    >
      <span
        v-for="bar in bars"
        :key="bar"
        class="w-24 rounded-[20px]"
        :class="bar"
      />
    </div>
    <footer
      class="relative z-10 flex items-center gap-2 px-6 py-5 text-sm text-muted sm:px-12 lg:px-24"
    >
      <span>{{ host }}</span>
      <template v-if="runtimeConfig.version">
        <span aria-hidden="true">·</span>
        <span>{{ t('shell.version', { version: runtimeConfig.version }) }}</span>
      </template>
    </footer>
  </div>
</template>
