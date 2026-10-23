<script setup lang="ts">
import { useI18n } from '~/plugins/i18n'

/** A value that is not there: 0 events, a bounce's span, no entry page. A faint dash, read as "none". */
const { t } = useI18n()
</script>

<template>
  <span
    class="text-faint"
    :aria-label="t('states.none')"
    >–</span
  >
</template>
