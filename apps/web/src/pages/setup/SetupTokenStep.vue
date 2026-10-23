<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { SetupCheck } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

const emit = defineEmits<{ accepted: [token: string, check: SetupCheck] }>()

const { t } = useI18n()
const { message } = useErrorMessage()

const token = ref('')
const check = useMutation({
  mutationFn: (value: string) => setupResource.check({ token: value }),
  onSuccess: (result, value) => emit('accepted', value, result),
})

onMounted(() => document.getElementById('setup-token')?.focus())
</script>

<template>
  <form
    class="grid gap-7"
    @submit.prevent="check.mutate(token.trim())"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.token.title', { name: runtimeConfig.instance_name }) }}
      <span class="block font-medium text-muted">{{ t('setup.token.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <FormField
        id="setup-token"
        v-model="token"
        autocomplete="off"
        :label="t('setup.token.label')"
        :description="t('setup.token.hint')"
        required
      />
      <Alert
        v-if="check.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(check.error.value) }}</p>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="check.isPending.value"
      >
        {{ t('setup.continue') }}
      </Button>
    </div>
  </form>
</template>
