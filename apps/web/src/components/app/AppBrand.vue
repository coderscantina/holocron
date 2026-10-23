<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppBrand.vue. The name is the
// installation's, from the runtime config.
import Icon from '~/components/Icon.vue'
import { runtimeConfig } from '~/services/runtime-config'

defineProps<{
  /** Rail mode: the mark alone, with the name kept for screen readers. */
  compact?: boolean
}>()
</script>

<template>
  <RouterLink
    :to="{ name: 'home' }"
    class="flex h-8 min-w-0 items-center gap-2 rounded-md px-2.5 text-sm font-semibold text-sidebar-accent-foreground transition-colors duration-200 ease-butter hover:bg-sidebar-accent"
    :class="compact && 'w-8 justify-center px-0'"
  >
    <span class="grid size-5 shrink-0 place-items-center rounded-[5px] bg-accent text-accent-foreground">
      <Icon
        name="lucide:chart-column"
        size="13"
        aria-hidden="true"
      />
    </span>
    <span
      class="truncate"
      :class="compact && 'sr-only'"
    >
      {{ runtimeConfig.instance_name }}
    </span>
  </RouterLink>
</template>
