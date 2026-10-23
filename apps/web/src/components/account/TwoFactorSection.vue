<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/TwoFactorSection.vue. Adds
// re-enrollment and the requirement policy; sensitive steps go through the reauthentication dialog.
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import RecoveryCodes from '~/components/account/RecoveryCodes.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import TwoFactorSetup from '~/components/account/TwoFactorSetup.vue'
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Skeleton } from '~/components/ui/skeleton'
import { useTwoFactorStatus } from '~/composables/useAccount'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const reauth = useReauth()
const status = useTwoFactorStatus()

const setupOpen = ref(false)
const codesOpen = ref(false)
const codes = ref<string[] | null>(null)
const busy = ref(false)

const enrolled = computed(() => status.data.value?.enrolled ?? false)
const required = computed(() => status.data.value?.required ?? false)
const enrolledAt = computed(() => {
  const at = status.data.value?.enrolled_at
  return at ? new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium' }).format(at) : ''
})

const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.twoFactor })

const regenerate = async () => {
  const ok = await confirm({
    title: t('twoFactor.regenerate'),
    message: t('twoFactor.regenerateConfirm'),
    confirmLabel: t('twoFactor.regenerate'),
  })
  if (!ok) return
  busy.value = true
  try {
    const result = await reauth.run('twoFactor.regenerateRecoveryCodes', {}, () => accountResource.regenerateRecoveryCodes())
    if (!result) return
    codes.value = result.secret
    codesOpen.value = true
    await refresh()
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}

const disable = async () => {
  const ok = await confirm({
    title: t('twoFactor.disable'),
    message: t('twoFactor.disableConfirm'),
    confirmLabel: t('twoFactor.disable'),
    variant: 'destructive',
  })
  if (!ok) return
  busy.value = true
  try {
    const result = await reauth.run('twoFactor.disable', {}, () => accountResource.disableTwoFactor())
    if (!result) return
    queryClient.setQueryData(queryKeys.twoFactor, result)
    toast.success(t('twoFactor.disabled'))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}

const onEnrolled = () => {
  void refresh()
  toast.success(t('twoFactor.enabled'))
}
</script>

<template>
  <SettingsSection
    id="two-factor"
    :title="t('twoFactor.title')"
    :description="t('twoFactor.description')"
  >
    <Skeleton
      v-if="status.isPending.value"
      class="h-20 w-full rounded-panel"
    />
    <div
      v-else
      class="flex flex-wrap items-center justify-between gap-3 rounded-panel border border-hairline bg-card p-4"
    >
      <div class="flex min-w-0 items-center gap-3">
        <span
          class="grid size-9 shrink-0 place-items-center rounded-lg"
          :class="enrolled ? 'bg-success-background/15 text-success' : 'bg-fill text-muted'"
        >
          <Icon
            :name="enrolled ? 'lucide:shield-check' : 'lucide:shield'"
            size="18"
            aria-hidden="true"
          />
        </span>
        <div class="grid min-w-0 gap-0.5">
          <div class="flex flex-wrap items-center gap-2 text-sm font-medium text-primary">
            {{ t('twoFactor.authenticator') }}
            <Badge
              :variant="enrolled ? 'success' : 'secondary'"
              size="xs"
            >
              {{ enrolled ? t('twoFactor.on') : t('twoFactor.off') }}
            </Badge>
            <Badge
              v-if="required"
              variant="warning"
              size="xs"
            >
              {{ t('twoFactor.required') }}
            </Badge>
          </div>
          <p class="text-xs text-pretty text-muted">
            {{
              enrolled
                ? t('twoFactor.onSince', { date: enrolledAt, left: status.data.value?.recovery_codes_left ?? 0 })
                : t('twoFactor.offHint')
            }}
          </p>
        </div>
      </div>
      <div class="flex flex-wrap gap-2">
        <Button
          v-if="!enrolled"
          variant="primary"
          @click="setupOpen = true"
        >
          {{ t('twoFactor.enable') }}
        </Button>
        <template v-else>
          <Button
            size="sm"
            :loading="busy"
            @click="regenerate"
          >
            {{ t('twoFactor.regenerate') }}
          </Button>
          <Button
            size="sm"
            variant="outline"
            @click="setupOpen = true"
          >
            {{ t('twoFactor.reenroll') }}
          </Button>
          <Button
            v-if="!required"
            variant="destructive"
            size="sm"
            :loading="busy"
            @click="disable"
          >
            {{ t('twoFactor.disable') }}
          </Button>
        </template>
      </div>
    </div>

    <Dialog v-model:open="setupOpen">
      <DialogContent
        class="max-w-md"
        @interact-outside="(event) => event.preventDefault()"
      >
        <DialogHeaderCombined
          :title="enrolled ? t('twoFactor.reenroll') : t('twoFactor.enable')"
          :description="t('twoFactor.setup.description')"
        />
        <TwoFactorSetup
          v-if="setupOpen"
          cancellable
          @enrolled="onEnrolled"
          @done="setupOpen = false"
          @cancel="setupOpen = false"
        />
      </DialogContent>
    </Dialog>

    <Dialog v-model:open="codesOpen">
      <DialogContent class="max-w-md">
        <DialogHeaderCombined
          :title="t('twoFactor.regenerate')"
          :description="t('twoFactor.regeneratedDescription')"
        />
        <RecoveryCodes :codes="codes" />
      </DialogContent>
    </Dialog>
  </SettingsSection>
</template>
