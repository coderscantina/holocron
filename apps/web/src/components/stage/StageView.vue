<script setup lang="ts">
import { useFullscreen } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useI18n } from '~/plugins/i18n'

/**
 * The stage layout of the visual views: a narrow column with the headline figure, a small
 * chart and a short list (`rail`), beside one visualization filling the rest of the canvas
 * (default slot). Below `lg` the visualization sits above the column. `controls` go to the
 * bottom right corner of the visualization, next to the fullscreen toggle.
 */
const props = withDefaults(defineProps<{ label: string; fullscreen?: boolean }>(), { fullscreen: true })

const { t } = useI18n()

const root = useTemplateRef('root')
const screen = useFullscreen(root)
</script>

<template>
  <div
    ref="root"
    class="grid gap-6 lg:grid-cols-[minmax(16rem,22rem)_1fr] lg:gap-0"
    :class="
      screen.isFullscreen.value
        ? 'h-svh bg-surface p-shell-page-x'
        : 'lg:h-[calc(100svh-var(--shell-header-height)-var(--shell-canvas-inset)-var(--shell-gutter)-5.5rem)] lg:min-h-[34rem]'
    "
  >
    <section
      class="flex min-h-0 min-w-0 flex-col lg:border-r lg:border-hairline lg:pr-8"
      :aria-label="label"
    >
      <slot name="rail" />
    </section>

    <div class="relative order-first min-h-[22rem] w-full min-w-0 lg:order-none lg:h-full lg:pl-6">
      <slot />
      <div
        v-if="$slots.controls || (props.fullscreen && screen.isSupported.value)"
        class="absolute right-0 bottom-0 flex gap-1"
      >
        <slot name="controls" />
        <SimpleTooltip
          v-if="props.fullscreen && screen.isSupported.value"
          :tooltip="screen.isFullscreen.value ? t('stage.exitFullscreen') : t('stage.fullscreen')"
          as-child
        >
          <Button
            variant="ghost"
            size="icon"
            class="text-muted hover:text-primary"
            :aria-label="screen.isFullscreen.value ? t('stage.exitFullscreen') : t('stage.fullscreen')"
            @click="screen.toggle()"
          >
            <Icon
              :name="screen.isFullscreen.value ? 'lucide:minimize-2' : 'lucide:maximize-2'"
              aria-hidden="true"
            />
          </Button>
        </SimpleTooltip>
      </div>
    </div>
  </div>
</template>
