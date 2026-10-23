<script setup lang="ts">
// From Kit (f447e797, MIT): resources/js/components/app/AppSidebarResizer.vue.
import { useEventListener } from '@vueuse/core'

import { useSidebar } from '~/composables/useSidebar'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const sidebar = useSidebar()

const dragging = defineModel<boolean>('dragging', { default: false })

const STEP = 16

const stop = () => {
  if (!dragging.value) return
  dragging.value = false
  document.body.style.removeProperty('user-select')
  document.body.style.removeProperty('cursor')
}

const start = (event: PointerEvent) => {
  event.preventDefault()
  dragging.value = true
  // Without this the drag selects the nav labels it passes over, and the
  // cursor flickers back to the default over every child element.
  document.body.style.userSelect = 'none'
  document.body.style.cursor = 'col-resize'
}

useEventListener(window, 'pointermove', (event: PointerEvent) => {
  if (!dragging.value) return
  sidebar.setWidth(event.clientX)
})

useEventListener(window, 'pointerup', stop)
useEventListener(window, 'pointercancel', stop)

/** Arrow keys move it too: a drag handle nobody can reach is decoration. */
const onKeydown = (event: KeyboardEvent) => {
  if (event.key === 'ArrowLeft') sidebar.setWidth(sidebar.width.value - STEP)
  else if (event.key === 'ArrowRight') sidebar.setWidth(sidebar.width.value + STEP)
  else if (event.key === 'Home' || event.key === 'End') sidebar.resetWidth()
  else return

  event.preventDefault()
}
</script>

<template>
  <div
    role="separator"
    aria-orientation="vertical"
    tabindex="0"
    :aria-label="t('shell.resizeSidebar')"
    :aria-valuenow="sidebar.width.value"
    :aria-valuemin="sidebar.bounds.min"
    :aria-valuemax="sidebar.bounds.max"
    class="absolute inset-y-0 -right-1 z-10 w-2 cursor-col-resize touch-none after:absolute after:inset-y-0 after:left-1/2 after:w-px after:-translate-x-1/2 after:bg-accent-text after:opacity-0 after:transition-opacity after:duration-200 hover:after:opacity-100 focus-visible:after:opacity-100"
    :class="dragging && 'after:opacity-100'"
    @pointerdown="start"
    @dblclick="sidebar.resetWidth()"
    @keydown="onKeydown"
  />
</template>
