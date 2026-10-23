<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/app/AppUserMenu.vue. Laravel auth and
// abilities are replaced by the `auth.session` resource; the PWA install entry is dropped. Appearance
// and language choices are saved to the account's preferences.
import Icon from '~/components/Icon.vue'
import { Avatar } from '~/components/ui/avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { colorModes, type ColorMode, useColorMode } from '~/composables/useColorMode'
import { usePreferences } from '~/composables/usePreferences'
import { useSession, useSignOut } from '~/composables/useSession'
import { useShortcutHelp } from '~/composables/useShortcuts'
import { initials } from '~/lib/utils'
import { isSupportedLocale, useI18n, type MessageKey } from '~/plugins/i18n'

const { t, locale, locales, setLocale } = useI18n()
const { account, isInstallAdmin } = useSession()
const preferences = usePreferences()
const signOut = useSignOut()
const { mode, isDark, setMode } = useColorMode()
const shortcutHelp = useShortcutHelp()

const colorModeIcons: Record<ColorMode, string> = {
  light: 'lucide:sun',
  dark: 'lucide:moon',
  system: 'lucide:monitor',
}

const colorModeLabels: Record<ColorMode, MessageKey> = {
  light: 'colorMode.light',
  dark: 'colorMode.dark',
  system: 'colorMode.system',
}

const onMode = (value: unknown) => {
  const next = colorModes.find((entry) => entry === value)
  if (!next) return
  setMode(next)
  preferences.save({ theme: next }).catch(() => {})
}

const onLocale = (value: unknown) => {
  if (typeof value !== 'string' || !isSupportedLocale(value)) return
  setLocale(value)
  preferences.save({ locale: value }).catch(() => {})
}
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger
      class="grid size-shell-control shrink-0 cursor-pointer place-items-center rounded-full bg-fill transition-colors duration-200 ease-butter hover:bg-fill-strong data-[state=open]:bg-fill-strong"
      :aria-label="t('shell.userMenu', { initials: initials(account?.name ?? '?') })"
    >
      <Avatar
        class="size-full rounded-full bg-transparent text-secondary-foreground"
        :name="account?.name ?? '?'"
      />
    </DropdownMenuTrigger>

    <DropdownMenuContent
      align="end"
      class="w-64"
    >
      <div class="flex items-center gap-3 px-2 py-2">
        <Avatar
          :name="account?.name ?? '?'"
          class="shrink-0 rounded-lg"
        />
        <div class="min-w-0">
          <p class="truncate text-sm font-semibold text-primary">{{ account?.name }}</p>
          <p class="truncate text-xs text-muted">{{ account?.email }}</p>
        </div>
      </div>
      <DropdownMenuSeparator />

      <DropdownMenuGroup>
        <DropdownMenuItem as-child>
          <RouterLink :to="{ name: 'account-profile' }">
            <Icon name="lucide:user" />
            {{ t('account.nav.profile') }}
          </RouterLink>
        </DropdownMenuItem>
        <DropdownMenuItem as-child>
          <RouterLink :to="{ name: 'account-security' }">
            <Icon name="lucide:shield" />
            {{ t('account.nav.security') }}
          </RouterLink>
        </DropdownMenuItem>
        <DropdownMenuItem as-child>
          <RouterLink :to="{ name: 'account-api-keys' }">
            <Icon name="lucide:key-round" />
            {{ t('account.nav.apiKeys') }}
          </RouterLink>
        </DropdownMenuItem>
        <DropdownMenuItem
          v-if="isInstallAdmin"
          as-child
        >
          <RouterLink :to="{ name: 'admin' }">
            <Icon name="lucide:server-cog" />
            {{ t('admin.title') }}
          </RouterLink>
        </DropdownMenuItem>
      </DropdownMenuGroup>
      <DropdownMenuSeparator />

      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <Icon :name="isDark ? colorModeIcons.dark : colorModeIcons.light" />
          {{ t('colorMode.label') }}
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            :model-value="mode"
            @update:model-value="onMode"
          >
            <DropdownMenuRadioItem
              v-for="option in colorModes"
              :key="option"
              :value="option"
            >
              <Icon :name="colorModeIcons[option]" />
              {{ t(colorModeLabels[option]) }}
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>

      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <Icon name="lucide:languages" />
          {{ t('shell.language') }}
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          <DropdownMenuRadioGroup
            :model-value="locale"
            @update:model-value="onLocale"
          >
            <DropdownMenuRadioItem
              v-for="entry in locales"
              :key="entry.code"
              :value="entry.code"
              :lang="entry.code"
            >
              {{ entry.name }}
            </DropdownMenuRadioItem>
          </DropdownMenuRadioGroup>
        </DropdownMenuSubContent>
      </DropdownMenuSub>

      <DropdownMenuItem
        class="max-lg:hidden"
        @select="shortcutHelp.show()"
      >
        <Icon name="lucide:keyboard" />
        {{ t('shortcuts.title') }}
        <DropdownMenuShortcut>?</DropdownMenuShortcut>
      </DropdownMenuItem>

      <DropdownMenuSeparator />
      <DropdownMenuItem @select="signOut()">
        <Icon name="lucide:log-out" />
        {{ t('auth.signOut') }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
