<script setup lang="ts">
import ShareShell from './ShareShell.vue'

/** `/share/:token` and its replays. Keyed by token, so another link starts from nothing. */
const route = useRoute()
const token = computed(() => String(route.params.token))
</script>

<template>
  <ShareShell
    :key="token"
    :token="token"
  />
</template>
