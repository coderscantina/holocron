<script setup lang="ts">
/**
 * The frame every settings page shares (site settings, administration, account): the sections
 * separated by rules, one document instead of a deck of cards. Pages put `SettingsSection`s in the
 * slot and nothing else. The page title row names the page, so `title` is only for a settings
 * page that has no title row of its own.
 */
defineProps<{
  title?: string
  description?: string
}>()
</script>

<template>
  <article class="grid gap-8">
    <header
      v-if="title"
      class="grid gap-1"
    >
      <h1 class="text-2xl font-semibold text-pretty text-primary">{{ title }}</h1>
      <p
        v-if="description"
        class="max-w-prose text-sm text-pretty text-muted"
      >
        {{ description }}
      </p>
    </header>

    <div class="divide-y divide-divider">
      <slot />
    </div>
  </article>
</template>
