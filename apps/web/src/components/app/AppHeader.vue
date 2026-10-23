<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppHeader.vue. Breadcrumbs and the
// notification bell are replaced by the workspace / site context chips and the scope outlet.
import AppBrand from '~/components/app/AppBrand.vue'
import AppContextSwitcher from '~/components/app/AppContextSwitcher.vue'
import { useTopBarScope } from '~/components/app/AppTopBarSlot'
import AppUserMenu from '~/components/app/AppUserMenu.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '~/components/ui/popover'
import { useI18n } from '~/plugins/i18n'

/**
 * The top bar sits on the frame, above the canvas: context chips, a divider, the page's scope
 * chips (see `AppTopBarSlot`) and the user menu. It stays one row; scope chips collapse first.
 */
const { t } = useI18n()

const scopeArea = ref<HTMLElement | null>(null)
const scope = useTopBarScope(scopeArea)
</script>

<template>
  <header
    data-frame
    class="@container sticky top-0 z-20 flex h-shell-header shrink-0 items-center gap-2 bg-sidebar px-3 pt-safe lg:static lg:pr-shell-inset lg:pl-0"
  >
    <AppBrand
      compact
      class="lg:hidden"
    />

    <AppContextSwitcher class="min-w-0" />

    <span
      v-if="scope.summary.value !== null"
      class="h-5.5 w-px shrink-0 bg-fill-strong"
      aria-hidden="true"
    />

    <div
      ref="scopeArea"
      class="flex min-w-0 flex-1 items-center gap-2 overflow-x-clip"
      :class="scope.collapsed.value && '@max-md:min-w-shell-control'"
    >
      <div
        v-show="!scope.collapsed.value"
        :ref="scope.setInline"
        class="flex w-max items-center gap-2"
      />
      <Popover v-if="scope.collapsed.value && scope.summary.value !== null">
        <PopoverTrigger as-child>
          <Button
            variant="fill"
            size="control"
            class="min-w-0 shrink font-medium @max-md:size-shell-control @max-md:shrink-0 @max-md:px-0"
            :aria-label="t('shell.scope', { summary: scope.summary.value })"
          >
            <Icon
              name="lucide:sliders-horizontal"
              class="text-muted"
              aria-hidden="true"
            />
            <span class="hidden truncate @md:inline">{{ scope.summary.value }}</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          class="w-auto max-w-[calc(100vw-2rem)] p-2"
        >
          <div
            :ref="scope.setOverflow"
            class="flex flex-col items-start gap-2"
          />
        </PopoverContent>
      </Popover>
    </div>

    <AppUserMenu />
  </header>
</template>
