<script setup lang="ts">
import { useI18n } from '~/plugins/i18n'

/**
 * The credit DB-IP's licence (CC BY 4.0) asks for on pages that show its locations (decision
 * 0018). Callers render it only while `runtimeConfig.dbip_attribution` is set.
 */
const { t } = useI18n()
</script>

<template>
  <a
    href="https://db-ip.com"
    target="_blank"
    rel="noopener"
    class="underline-offset-4 hover:underline"
  >
    {{ t('shell.dbipAttribution') }}
  </a>
</template>
