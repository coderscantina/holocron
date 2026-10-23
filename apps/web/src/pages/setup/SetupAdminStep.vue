<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { applyLoginResult } from '~/composables/useSession'
import { PASSWORD_MIN_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { serverStarted } from '~/services/runtime-config'

import type { SetupPath } from './steps'

const props = defineProps<{ token: string; path: SetupPath }>()
const emit = defineEmits<{ created: [] }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const email = ref('')
const password = ref('')


/** The import path stays in setup mode; only the fresh start hands over to the server now. */
const fresh = computed(() => props.path !== 'import')

const create = useMutation({
  // The login clears every cached query, so it goes first: the site step mounts as soon as the
  // server reports it left setup mode, and its queries must survive.
  mutationFn: async () => {
    const path = fresh.value ? 'fresh' : 'import'
    applyLoginResult(
      await setupResource.admin({ token: props.token, email: email.value, password: password.value, path })
    )
    if (fresh.value) await serverStarted()
  },
  onSuccess: () => emit('created'),
})

const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))

onMounted(() => document.getElementById('setup-email')?.focus())
</script>

<template>
  <form
    class="grid gap-7"
    novalidate
    @submit.prevent="create.mutate()"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.admin.title') }}
      <span class="block font-medium text-muted">{{ t('setup.admin.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <FormField
        id="setup-email"
        v-model="email"
        type="email"
        autocomplete="username"
        :label="t('auth.email')"
        :error="fieldMessage(error, 'email')"
        required
      />
      <FormField
        id="setup-password"
        v-model="password"
        type="password"
        autocomplete="new-password"
        :label="t('auth.password')"
        :description="t('setup.admin.passwordHint', { min: PASSWORD_MIN_CHARS })"
        :error="fieldMessage(error, 'password')"
        required
      />
      <CommandAlert
        :shown="['email', 'password']"
        :error="error"
      />
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="create.isPending.value"
        :disabled="!email.trim() || !password"
      >
        {{ create.isPending.value && fresh ? t('setup.admin.starting') : t('setup.admin.create') }}
      </Button>
    </div>
  </form>
</template>
