<script setup lang="ts">
import { toast } from 'vue-sonner'

import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Label } from '~/components/ui/label'
import { RadioGroup, RadioGroupItem } from '~/components/ui/radio-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Skeleton } from '~/components/ui/skeleton'
import { accentColors, useAccentColor } from '~/composables/useAccentColor'
import { colorModes, useColorMode, type ColorMode } from '~/composables/useColorMode'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { usePreferences } from '~/composables/usePreferences'
import type { Preferences } from '~/generated'
import { rangePresets } from '~/lib/exploration-labels'
import { isSupportedLocale, useI18n, type MessageKey } from '~/plugins/i18n'
import { canonical } from '~/services/exploration'

/**
 * Display settings stored with the account, so every browser starts the same way. Each choice
 * saves at once; language, theme and accent also apply at once.
 */
const { t, locale, locales, setLocale } = useI18n()
const { message } = useErrorMessage()
const { setMode } = useColorMode()
const { accent, setAccent } = useAccentColor()
const { query, save } = usePreferences()
usePageMeta(() => t('account.preferences.title'))

const SYSTEM = 'system'
const timezones = Intl.supportedValuesOf('timeZone')
const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone

const themeLabels: Record<ColorMode, MessageKey> = {
  light: 'colorMode.light',
  dark: 'colorMode.dark',
  system: 'colorMode.system',
}

const store = async (patch: Preferences) => {
  try {
    await save(patch)
    toast.success(t('account.preferences.saved'))
  } catch (error) {
    toast.error(message(error))
  }
}

const theme = computed(() => query.data.value?.theme ?? SYSTEM)
const onTheme = (value: unknown) => {
  const next = colorModes.find((mode) => mode === value)
  if (!next) return
  setMode(next)
  void store({ theme: next })
}

const onAccent = (value: unknown) => {
  const next = accentColors.find((color) => color === value)
  if (!next) return
  setAccent(next)
  void store({ accent: next })
}

const language = computed(() => query.data.value?.locale ?? locale.value)
const onLanguage = (value: unknown) => {
  if (typeof value !== 'string' || !isSupportedLocale(value)) return
  setLocale(value)
  void store({ locale: value })
}

const timezone = computed(() => query.data.value?.timezone ?? SYSTEM)
const onTimezone = (value: unknown) => {
  if (typeof value !== 'string') return
  void store({ timezone: value === SYSTEM ? undefined : value })
}

const range = computed(() => {
  const current = query.data.value?.default_range
  return current ? canonical(current) : SYSTEM
})
const rangeLabel = computed(() => rangePresets.find((preset) => canonical(preset.range) === range.value)?.key)
const onRange = (value: unknown) => {
  const preset = rangePresets.find((entry) => canonical(entry.range) === value)
  void store({ default_range: preset?.range })
}
</script>

<template>
  <SettingsPage>
    <Skeleton
      v-if="query.isPending.value"
      class="h-64 w-full"
    />
    <template v-else>
      <SettingsSection
        id="appearance"
        :title="t('account.preferences.appearance')"
        :description="t('account.preferences.appearanceDescription')"
      >
        <div class="grid gap-1.5">
          <Label for="pref-theme">{{ t('colorMode.label') }}</Label>
          <Select
            :model-value="theme"
            @update:model-value="onTheme"
          >
            <SelectTrigger id="pref-theme">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="mode in colorModes"
                :key="mode"
                :value="mode"
              >
                {{ t(themeLabels[mode]) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-2">
          <Label
            id="pref-accent"
            as="span"
          >
            {{ t('accent.label') }}
          </Label>
          <RadioGroup
            :model-value="accent"
            aria-labelledby="pref-accent"
            orientation="horizontal"
            class="flex flex-wrap gap-2.5"
            @update:model-value="onAccent"
          >
            <RadioGroupItem
              v-for="color in accentColors"
              :key="color"
              :value="color"
              :data-accent="color"
              :aria-label="t(`accent.${color}`)"
              :title="t(`accent.${color}`)"
              class="grid h-8 w-8 place-items-center bg-accent text-accent-foreground shadow-none ring-offset-2 ring-offset-surface transition-shadow duration-200 hover:bg-accent-hover data-[state=checked]:ring-2 data-[state=checked]:ring-accent-text"
            />
          </RadioGroup>
        </div>
        <div class="grid gap-1.5">
          <Label for="pref-language">{{ t('shell.language') }}</Label>
          <Select
            :model-value="language"
            @update:model-value="onLanguage"
          >
            <SelectTrigger id="pref-language">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="entry in locales"
                :key="entry.code"
                :value="entry.code"
                :lang="entry.code"
              >
                {{ entry.name }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>
      <SettingsSection
        id="explorations"
        :title="t('account.preferences.explorations')"
        :description="t('account.preferences.explorationsDescription')"
      >
        <div class="grid gap-1.5">
          <Label for="pref-timezone">{{ t('account.preferences.timezone') }}</Label>
          <Select
            :model-value="timezone"
            @update:model-value="onTimezone"
          >
            <SelectTrigger id="pref-timezone">
              <SelectValue>
                {{ timezone === SYSTEM ? t('account.preferences.siteTimezone') : timezone }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent class="max-h-80">
              <SelectItem :value="SYSTEM">{{ t('account.preferences.siteTimezone') }}</SelectItem>
              <SelectItem
                v-for="zone in timezones"
                :key="zone"
                :value="zone"
              >
                {{ zone === browserZone ? t('account.preferences.browserZone', { zone }) : zone }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="grid gap-1.5">
          <Label for="pref-range">{{ t('account.preferences.range') }}</Label>
          <Select
            :model-value="range"
            @update:model-value="onRange"
          >
            <SelectTrigger id="pref-range">
              <SelectValue>
                {{ rangeLabel ? t(rangeLabel) : t('account.preferences.pageRange') }}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem :value="SYSTEM">{{ t('account.preferences.pageRange') }}</SelectItem>
              <SelectItem
                v-for="preset in rangePresets"
                :key="preset.key"
                :value="canonical(preset.range)"
              >
                {{ t(preset.key) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </SettingsSection>
    </template>
  </SettingsPage>
</template>
