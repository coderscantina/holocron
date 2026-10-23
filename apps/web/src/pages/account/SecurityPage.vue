<script setup lang="ts">
import PasswordSection from '~/components/account/PasswordSection.vue'
import SessionsSection from '~/components/account/SessionsSection.vue'
import SettingsPage from '~/components/account/SettingsPage.vue'
import TwoFactorSection from '~/components/account/TwoFactorSection.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
usePageMeta(() => t('account.security.title'))
</script>

<template>
  <SettingsPage>
    <PasswordSection />
    <TwoFactorSection />
    <SessionsSection />
  </SettingsPage>
</template>
