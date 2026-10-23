<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { applyLoginResult } from '~/composables/useSession'
import { useI18n } from '~/plugins/i18n'

/** Signs the install admin in again while an Umami import waits, for example after a restart. */
const emit = defineEmits<{ 'signed-in': [] }>()

const { t } = useI18n()
const { message } = useErrorMessage()

const email = ref('')
const password = ref('')

const signIn = useMutation({
  mutationFn: async () => {
    applyLoginResult(await setupResource.signIn({ email: email.value, password: password.value }))
  },
  onSuccess: () => emit('signed-in'),
})

onMounted(() => document.getElementById('setup-sign-in-email')?.focus())
</script>

<template>
  <form
    class="grid gap-7"
    novalidate
    @submit.prevent="signIn.mutate()"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.signIn.title') }}
      <span class="block font-medium text-muted">{{ t('setup.signIn.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <FormField
        id="setup-sign-in-email"
        v-model="email"
        type="email"
        autocomplete="username"
        :label="t('auth.email')"
        required
      />
      <FormField
        id="setup-sign-in-password"
        v-model="password"
        type="password"
        autocomplete="current-password"
        :label="t('auth.password')"
        :description="t('setup.signIn.hint')"
        required
      />
      <Alert
        v-if="signIn.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(signIn.error.value) }}</p>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="signIn.isPending.value"
        :disabled="!email.trim() || !password"
      >
        {{ t('auth.signIn') }}
      </Button>
    </div>
  </form>
</template>
