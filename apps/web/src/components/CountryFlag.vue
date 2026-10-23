<script setup lang="ts">
/**
 * The flag of a country by ISO 3166-1 alpha-2 code, shown in front of its name. Decorative: the
 * name next to it says the country. Unknown codes render nothing. The SVGs (country-flag-icons,
 * MIT) are files of their own, so a page loads only the flags it shows.
 */
const props = defineProps<{ code: string }>()

const src = computed(() => FLAGS[props.code.toUpperCase()] ?? null)
</script>

<script lang="ts">
const urls = import.meta.glob<string>('/node_modules/country-flag-icons/3x2/[A-Z][A-Z].svg', {
  eager: true,
  query: '?url',
  import: 'default',
})

const FLAGS: Record<string, string> = Object.fromEntries(
  Object.entries(urls).map(([path, url]) => [path.slice(-6, -4), url])
)
</script>

<template>
  <img
    v-if="src"
    :src="src"
    alt=""
    width="18"
    height="12"
    loading="lazy"
    decoding="async"
    class="inline-block h-3 w-4.5 shrink-0 rounded-[2px] object-cover ring-1 ring-hairline"
  />
</template>
