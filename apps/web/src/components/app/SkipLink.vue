<script setup lang="ts">
// From Kit (f447e797, MIT): resources/js/components/app/SkipLink.vue.
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
</script>

<template>
  <a
    href="#main-content"
    class="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-lg focus:bg-popover focus:px-3 focus:py-2 focus:text-sm focus:font-semibold focus:text-primary focus:shadow-soft-lg"
  >
    {{ t('shell.skipToContent') }}
  </a>
</template>
