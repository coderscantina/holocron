<script setup lang="ts">
import DialogDescription from './DialogDescription.vue'
import DialogHeader from './DialogHeader.vue'
import DialogTitle from './DialogTitle.vue'

defineProps<{
  title?: string
  description?: string
}>()
</script>

<template>
  <DialogHeader>
    <DialogTitle v-if="title">{{ title }}</DialogTitle>
    <DialogDescription v-if="description">{{ description }}</DialogDescription>
  </DialogHeader>
</template>
