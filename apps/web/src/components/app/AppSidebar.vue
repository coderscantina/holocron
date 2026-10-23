<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppSidebar.vue.
import AppBrand from '~/components/app/AppBrand.vue'
import AppJumpField from '~/components/app/AppJumpField.vue'
import AppSidebarNav from '~/components/app/AppSidebarNav.vue'
import AppSidebarResizer from '~/components/app/AppSidebarResizer.vue'
import DbipAttribution from '~/components/DbipAttribution.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

const { t } = useI18n()
const sidebar = useSidebar()
const dragging = ref(false)
</script>

<template>
  <aside
    id="app-sidebar"
    data-frame
    class="relative hidden shrink-0 flex-col bg-sidebar text-sidebar-foreground select-none lg:flex"
    :class="!dragging && 'transition-[width] duration-200 ease-butter'"
    :style="{ width: `${sidebar.currentWidth.value}px` }"
  >
    <div
      class="flex shrink-0 items-center gap-1"
      :class="sidebar.collapsed.value ? 'flex-col px-3 py-2' : 'h-shell-header px-4'"
    >
      <AppBrand :compact="sidebar.collapsed.value" />
      <Button
        v-if="sidebar.collapsible"
        variant="ghost"
        size="icon"
        class="text-muted"
        :class="!sidebar.collapsed.value && 'ml-auto'"
        :aria-label="t('shell.toggleSidebar')"
        :aria-expanded="!sidebar.collapsed.value"
        aria-controls="app-sidebar"
        @click="sidebar.toggle()"
      >
        <Icon :name="sidebar.collapsed.value ? 'lucide:panel-left-open' : 'lucide:panel-left-close'" />
      </Button>
    </div>

    <div
      class="shrink-0 pb-2"
      :class="sidebar.collapsed.value ? 'px-3' : 'px-4 pt-2'"
    >
      <AppJumpField :compact="sidebar.collapsed.value" />
    </div>

    <AppSidebarNav :compact="sidebar.collapsed.value" />

    <p
      v-if="runtimeConfig.dbip_attribution && !sidebar.collapsed.value"
      class="shrink-0 truncate px-5 pb-1 text-2xs text-sidebar-muted"
    >
      <DbipAttribution />
    </p>
    <p
      v-if="runtimeConfig.version && !sidebar.collapsed.value"
      class="shrink-0 truncate px-5 pb-3 text-2xs text-sidebar-muted"
    >
      {{ t('shell.version', { version: runtimeConfig.version }) }}
    </p>

    <AppSidebarResizer
      v-if="sidebar.resizable && !sidebar.collapsed.value"
      v-model:dragging="dragging"
    />
  </aside>
</template>
