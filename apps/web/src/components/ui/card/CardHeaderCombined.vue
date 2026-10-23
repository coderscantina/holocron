<script setup lang="ts">
import CardDescription from './CardDescription.vue'
import CardHeader from './CardHeader.vue'
import CardTitle from './CardTitle.vue'

defineProps<{
  title?: string
  description?: string
}>()
</script>

<template>
  <CardHeader>
    <CardTitle v-if="title">{{ title }}</CardTitle>
    <CardDescription v-if="description">{{ description }}</CardDescription>
  </CardHeader>
</template>
