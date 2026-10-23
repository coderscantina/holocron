<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/TwoFactorSetupDialog.vue. The
// steps render without a dialog, so the enrollment-only page can show them in place.
import QRCode from 'qrcode'

import { accountResource } from '~/api/resources/account'
import RecoveryCodes from '~/components/account/RecoveryCodes.vue'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Checkbox } from '~/components/ui/checkbox'
import { Input } from '~/components/ui/input'
import { Label } from '~/components/ui/label'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import type { TotpSecret } from '~/generated'
import { TOTP_DIGITS } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

/**
 * Three steps: what this is, scan and verify, then the recovery codes. The codes step cannot be
 * left until the person says they kept them, because it is the only time they are shown. A
 * running factor stays active until the new one is confirmed, so this also re-enrolls.
 */
const emit = defineEmits<{
  /** The factor is confirmed; the session is a full one from here on. */
  enrolled: []
  done: []
  cancel: []
}>()
defineProps<{ cancellable?: boolean }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const reauth = useReauth()

type Step = 'intro' | 'scan' | 'codes'

const step = ref<Step>('intro')
const secret = ref<TotpSecret | null>(null)
const withheld = ref(false)
const qr = ref<string | null>(null)
const code = ref('')
const codes = ref<string[] | null>(null)
// reka-ui's checkbox models a third, indeterminate state.
const kept = ref<boolean | 'indeterminate'>(false)
const busy = ref(false)
const error = ref<unknown>(null)

const start = async () => {
  busy.value = true
  error.value = null
  try {
    const result = await reauth.run('twoFactor.begin', {}, () => accountResource.beginTwoFactor())
    if (!result) return
    secret.value = result.secret
    withheld.value = result.secret_withheld
    qr.value = result.secret ? await QRCode.toDataURL(result.secret.uri, { margin: 2, width: 320 }) : null
    step.value = 'scan'
  } catch (failure) {
    error.value = failure
  } finally {
    busy.value = false
  }
}

const confirm = async () => {
  busy.value = true
  error.value = null
  try {
    const args = { code: code.value.trim() }
    const result = await reauth.run('twoFactor.confirm', args, () => accountResource.confirmTwoFactor(args.code))
    if (!result) return
    codes.value = result.secret
    step.value = 'codes'
    emit('enrolled')
  } catch (failure) {
    error.value = failure
    code.value = ''
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div
    v-if="step === 'intro'"
    class="grid gap-4"
  >
    <ol class="grid list-decimal gap-1 pl-5 text-sm text-muted">
      <li>{{ t('twoFactor.setup.step1') }}</li>
      <li>{{ t('twoFactor.setup.step2') }}</li>
      <li>{{ t('twoFactor.setup.step3', { digits: TOTP_DIGITS }) }}</li>
    </ol>
    <Alert
      v-if="error"
      color="destructive"
    >
      <p class="text-sm">{{ message(error) }}</p>
    </Alert>
    <div class="flex justify-end gap-2">
      <Button
        v-if="cancellable"
        type="button"
        variant="ghost"
        @click="emit('cancel')"
      >
        {{ t('actions.cancel') }}
      </Button>
      <Button
        variant="primary"
        :loading="busy"
        @click="start"
      >
        {{ t('actions.continue') }}
      </Button>
    </div>
  </div>

  <form
    v-else-if="step === 'scan'"
    class="grid gap-4"
    @submit.prevent="confirm"
  >
    <template v-if="secret">
      <img
        v-if="qr"
        :src="qr"
        :alt="t('twoFactor.setup.qr')"
        class="mx-auto size-48 rounded-lg border border-border bg-white p-2"
      />
      <div class="grid gap-1.5">
        <Label for="totp-secret">{{ t('twoFactor.setup.manualEntry') }}</Label>
        <Input
          id="totp-secret"
          :model-value="secret.key"
          readonly
          class="text-center font-mono tracking-widest"
        />
      </div>
      <FormField
        id="totp-code"
        v-model="code"
        autocomplete="one-time-code"
        inputmode="numeric"
        :label="t('twoFactor.code')"
        :description="t('twoFactor.setup.codeHint', { digits: TOTP_DIGITS })"
        required
      />
    </template>
    <Alert
      v-else-if="withheld"
      color="info"
    >
      <p class="text-sm">{{ t('twoFactor.setup.withheld') }}</p>
    </Alert>
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
        @click="step = 'intro'"
      >
        {{ t('actions.back') }}
      </Button>
      <Button
        v-if="secret"
        variant="primary"
        type="submit"
        :loading="busy"
        :disabled="code.trim().length < TOTP_DIGITS"
      >
        {{ t('twoFactor.setup.confirm') }}
      </Button>
    </div>
  </form>

  <div
    v-else
    class="grid gap-4"
  >
    <RecoveryCodes :codes="codes" />
    <div class="flex items-center gap-2">
      <Checkbox
        id="codes-kept"
        v-model="kept"
      />
      <Label for="codes-kept">{{ t('twoFactor.codesKept') }}</Label>
    </div>
    <div class="flex justify-end">
      <Button
        variant="primary"
        :disabled="codes !== null && kept !== true"
        @click="emit('done')"
      >
        {{ t('actions.done') }}
      </Button>
    </div>
  </div>
</template>
