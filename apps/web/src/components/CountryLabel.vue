<script setup lang="ts">
import CountryFlag from '~/components/CountryFlag.vue'
import { useExplorationFormat } from '~/composables/useExplorationFormat'

/** A country as its flag and localized name, from an ISO 3166-1 alpha-2 code. */
const props = defineProps<{ code: string }>()

const format = useExplorationFormat()
const name = computed(() => format.textLabel('country', props.code))
</script>

<template>
  <span class="inline-flex min-w-0 items-center gap-2">
    <CountryFlag :code="code" />
    <span class="truncate">{{ name }}</span>
  </span>
</template>
