<script setup lang="ts">
/**
 * Mounts the install admin's server card above Site settings in the sidebar. ServerCard renders
 * nothing for anyone who is not an install admin, so the sidebar reserves no space for it.
 */
import ServerCard from '~/components/admin/ServerCard.vue'
</script>

<template>
  <ServerCard />
</template>
