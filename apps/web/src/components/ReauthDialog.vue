<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/PasswordConfirmDialog.vue. Asks for
// the second factor too when the account has one, and stores the server's window in the session.
import { ApiError } from '~/api'
import { sessionResource } from '~/api/resources/session'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useTwoFactorStatus } from '~/composables/useAccount'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { patchSession } from '~/composables/useSession'
import type { SecondFactorProof } from '~/generated'
import { REAUTH_MINUTES } from '~/lib/reauth'
import { useI18n } from '~/plugins/i18n'

/**
 * The password check in front of sensitive commands. It often opens over another dialog (the
 * API key form), whose portal comes later in the document, so it sits one layer higher. Wrong input keeps the dialog open: a
 * failed check is a 401 that does not end the session.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const reauth = useReauth()
const twoFactor = useTwoFactorStatus()

const password = ref('')
const code = ref('')
const recovery = ref(false)
const askCode = ref(false)
const error = ref<unknown>(null)
const busy = ref(false)

const withCode = computed(() => askCode.value || twoFactor.data.value?.enrolled === true)

watch(reauth.open, (open) => {
  if (open) return
  password.value = ''
  code.value = ''
  recovery.value = false
  askCode.value = false
  error.value = null
})

const submit = async () => {
  busy.value = true
  error.value = null
  const second_factor: SecondFactorProof | undefined =
    withCode.value && code.value ? { kind: recovery.value ? 'recovery_code' : 'totp', code: code.value } : undefined
  try {
    const result = await sessionResource.reauthenticate({ password: password.value, second_factor })
    patchSession({ reauthenticated_until: result.reauthenticated_until })
    reauth.settle(true)
  } catch (failure) {
    if (failure instanceof ApiError && failure.code === 'second_factor_required') askCode.value = true
    code.value = ''
    error.value = failure
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog
    :open="reauth.open.value"
    @update:open="(value) => !value && reauth.settle(false)"
  >
    <DialogContent class="z-60 max-w-md">
      <DialogHeaderCombined
        :title="t('reauth.title')"
        :description="t('reauth.description', { minutes: REAUTH_MINUTES })"
      />
      <form
        class="grid gap-4"
        @submit.prevent="submit"
      >
        <FormField
          id="reauth-password"
          v-model="password"
          type="password"
          autocomplete="current-password"
          :label="t('auth.password')"
          required
        />
        <template v-if="withCode">
          <FormField
            id="reauth-code"
            v-model="code"
            :autocomplete="recovery ? 'off' : 'one-time-code'"
            :inputmode="recovery ? 'text' : 'numeric'"
            :label="recovery ? t('twoFactor.recoveryCode') : t('twoFactor.code')"
            required
          />
          <button
            type="button"
            class="w-fit cursor-pointer text-sm text-muted hover:text-primary hover:underline"
            @click="((recovery = !recovery), (code = ''))"
          >
            {{ recovery ? t('twoFactor.useApp') : t('twoFactor.useRecoveryCode') }}
          </button>
        </template>
        <Alert
          v-if="error"
          color="destructive"
        >
          <p class="text-sm">{{ message(error) }}</p>
        </Alert>
        <div class="flex justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            @click="reauth.settle(false)"
          >
            {{ t('actions.cancel') }}
          </Button>
          <Button
            type="submit"
            variant="primary"
            :loading="busy"
          >
            {{ t('actions.confirm') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
