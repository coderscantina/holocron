<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/PasswordSection.vue. The
// current password is asked by the reauthentication dialog, not by this form.
import { toast } from 'vue-sonner'

import { accountResource } from '~/api/resources/account'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const reauth = useReauth()

const password = ref('')
const confirmation = ref('')
const busy = ref(false)
const error = ref<Error | null>(null)

/** Checked here so a typo is caught while the person still looks at the field. */
const mismatch = computed(() =>
  confirmation.value.length > 0 && password.value !== confirmation.value ? t('account.security.mismatch') : null
)
const submittable = computed(
  () =>
    password.value.length >= PASSWORD_MIN_CHARS &&
    password.value.length <= PASSWORD_MAX_CHARS &&
    password.value === confirmation.value
)

const submit = async () => {
  busy.value = true
  error.value = null
  const args = { new_password: password.value }
  try {
    const done = await reauth.run('account.changePassword', args, () => accountResource.changePassword(args))
    if (!done) return
    password.value = ''
    confirmation.value = ''
    toast.success(t('account.security.passwordChanged'))
  } catch (failure) {
    error.value = failure instanceof Error ? failure : null
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SettingsSection
    id="password"
    :title="t('account.security.password')"
    :description="t('account.security.passwordDescription')"
  >
    <form
      id="password-form"
      class="grid gap-4"
      @submit.prevent="submit"
    >
      <input
        type="text"
        autocomplete="username"
        class="sr-only"
        tabindex="-1"
        aria-hidden="true"
      />
      <FormField
        id="new-password"
        v-model="password"
        type="password"
        autocomplete="new-password"
        :label="t('account.security.newPassword')"
        :description="t('account.security.passwordHint', { min: PASSWORD_MIN_CHARS })"
        :error="fieldMessage(error, 'new_password')"
        required
      />
      <FormField
        id="new-password-confirmation"
        v-model="confirmation"
        type="password"
        autocomplete="new-password"
        :label="t('account.security.confirmPassword')"
        :error="mismatch"
        required
      />
      <CommandAlert
          :shown="['new_password']" :error="error" />
    </form>
    <template #footer>
      <Button
        variant="primary"
        type="submit"
        form="password-form"
        :loading="busy"
        :disabled="!submittable"
      >
        {{ t('account.security.changePassword') }}
      </Button>
    </template>
  </SettingsSection>
</template>
