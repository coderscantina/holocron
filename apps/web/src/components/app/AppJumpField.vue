<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '~/components/ui/tooltip'
import { useCommandPalette } from '~/composables/useCommandPalette'
import { IS_MAC } from '~/lib/shortcuts'
import { useI18n } from '~/plugins/i18n'

/**
 * The sidebar's trigger for the command palette: a filled field with the shortcut hint, or in
 * the collapsed rail an icon button. The palette hands focus back to whatever held it when it
 * opened, so the trigger takes focus first; Safari does not focus a button on click.
 */
defineProps<{ compact?: boolean }>()

const emit = defineEmits<{ open: [] }>()

const { t } = useI18n()
const palette = useCommandPalette()

const open = (event: MouseEvent) => {
  if (event.currentTarget instanceof HTMLElement) event.currentTarget.focus()
  palette.open()
  emit('open')
}
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <Tooltip>
      <TooltipTrigger as-child>
        <button
          type="button"
          class="flex h-shell-nav w-full cursor-pointer items-center gap-3 rounded-control bg-fill pr-2 pl-3 text-[0.9375rem] text-muted transition-colors duration-150 ease-butter hover:bg-fill-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          :class="compact && 'justify-center px-0'"
          :aria-label="compact ? t('commandPalette.open') : undefined"
          :aria-keyshortcuts="IS_MAC ? 'Meta+K' : 'Control+K'"
          @click="open"
        >
          <Icon
            name="lucide:search"
            size="16"
            class="shrink-0"
            aria-hidden="true"
          />
          <template v-if="!compact">
            <span class="truncate">{{ t('commandPalette.jump') }}</span>
            <kbd class="ml-auto shrink-0 rounded-sm bg-fill-strong px-1.5 py-0.5 text-2xs">
              {{ IS_MAC ? t('commandPalette.hintMac') : t('commandPalette.hintOther') }}
            </kbd>
          </template>
        </button>
      </TooltipTrigger>
      <TooltipContent
        v-if="compact"
        side="right"
      >
        {{ t('commandPalette.jump') }}
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>
