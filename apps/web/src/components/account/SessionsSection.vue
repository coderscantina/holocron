<script setup lang="ts">
import { toast } from 'vue-sonner'

import { sessionResource } from '~/api/resources/session'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Button } from '~/components/ui/button'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { SESSION_IDLE_MS, SESSION_LIFETIME_MS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * Signs out every other browser. The server keeps no list of sessions to show, so this is one
 * action: a lost laptop or a shared computer ends at its next request.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const busy = ref(false)

const DAY_MS = 86_400_000

const revokeOthers = async () => {
  const ok = await confirm({
    title: t('sessions.revokeOthers'),
    message: t('sessions.revokeConfirm'),
    confirmLabel: t('sessions.revokeOthers'),
    variant: 'destructive',
  })
  if (!ok) return
  busy.value = true
  try {
    const { revoked } = await sessionResource.revokeOthers()
    toast.success(t('sessions.revoked', { count: revoked }))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SettingsSection
    id="sessions"
    :title="t('sessions.title')"
    :description="t('sessions.description', { idle: SESSION_IDLE_MS / DAY_MS, lifetime: SESSION_LIFETIME_MS / DAY_MS })"
  >
    <p class="text-sm text-muted">{{ t('sessions.hint') }}</p>
    <template #footer>
      <Button
        variant="destructive"
        :loading="busy"
        @click="revokeOthers"
      >
        {{ t('sessions.revokeOthers') }}
      </Button>
    </template>
  </SettingsSection>
</template>
