<script setup lang="ts">
import SheetDescription from './SheetDescription.vue'
import SheetHeader from './SheetHeader.vue'
import SheetTitle from './SheetTitle.vue'

defineProps<{
  title?: string
  description?: string
}>()
</script>

<template>
  <SheetHeader>
    <SheetTitle v-if="title">{{ title }}</SheetTitle>
    <SheetDescription v-if="description">{{ description }}</SheetDescription>
  </SheetHeader>
</template>
