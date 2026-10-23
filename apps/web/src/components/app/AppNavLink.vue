<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppNavLink.vue.
import Icon from '~/components/Icon.vue'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '~/components/ui/tooltip'
import type { NavigationItem } from '~/composables/useNavigation'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{
  item: NavigationItem
  /** Rail mode: icon only, label moved into a tooltip. */
  compact?: boolean
}>()

const emit = defineEmits<{ navigate: [] }>()

const { t } = useI18n()
const route = useRoute()
const label = computed(() => t(props.item.labelKey))
/** A detail page marks the item it belongs to, which `RouterLink` alone does not match. */
const section = computed(() => route.meta.section !== undefined && route.meta.section === props.item.id)

/** What the indicator says in words; the badge's own digits are hidden from screen readers. */
const indicatorText = computed(() => {
  const indicator = props.item.indicator
  if (!indicator) return null
  return indicator.kind === 'count'
    ? t(indicator.label, { count: indicator.value })
    : t('nav.indicator.live', { count: indicator.value })
})
</script>

<template>
  <TooltipProvider :delay-duration="300">
    <Tooltip>
      <TooltipTrigger as-child>
        <RouterLink
          :to="item.to"
          class="group/nav relative flex h-shell-nav w-full items-center gap-3 rounded-control px-3 text-[0.9375rem] font-medium text-sidebar-muted transition-colors duration-150 ease-butter hover:bg-fill"
          :class="[compact && 'justify-center px-0', section && 'bg-nav-active! text-nav-active-foreground!']"
          active-class="bg-nav-active! text-nav-active-foreground!"
          @click="emit('navigate')"
        >
          <Icon
            :name="item.icon"
            size="16"
            class="shrink-0 opacity-75 group-hover/nav:opacity-100"
            aria-hidden="true"
          />
          <span
            class="truncate"
            :class="compact && 'sr-only'"
          >
            {{ label }}<span
              v-if="indicatorText"
              class="sr-only"
            >{{ `, ${indicatorText}` }}</span>
          </span>
          <template v-if="item.indicator">
            <span
              v-if="compact"
              class="absolute top-1.5 right-2 size-2 rounded-full ring-2 ring-sidebar"
              :class="item.indicator.kind === 'count' ? 'bg-accent' : 'bg-success'"
              aria-hidden="true"
            />
            <span
              v-else-if="item.indicator.kind === 'count'"
              class="ml-auto grid h-5 min-w-5 place-items-center rounded-full bg-accent px-1.5 text-xs font-semibold text-accent-foreground tabular-nums"
              aria-hidden="true"
            >
              {{ item.indicator.value }}
            </span>
            <span
              v-else
              class="ml-auto flex items-center gap-1.5 text-xs font-medium text-success tabular-nums"
              aria-hidden="true"
            >
              <span class="size-1.5 rounded-full bg-success" />
              {{ item.indicator.value }}
            </span>
          </template>
        </RouterLink>
      </TooltipTrigger>
      <TooltipContent
        v-if="compact"
        side="right"
      >
        {{ label }}<template v-if="indicatorText">, {{ indicatorText }}</template>
      </TooltipContent>
    </Tooltip>
  </TooltipProvider>
</template>
