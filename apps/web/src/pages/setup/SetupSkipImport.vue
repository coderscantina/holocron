<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig, serverStarted } from '~/services/runtime-config'

/**
 * Leaves the import path for a fresh start. A started or finished import is discarded first,
 * confirmed by the instance name; then setup ends and the wizard continues with the first site.
 */
const props = defineProps<{ imported: boolean }>()
const emit = defineEmits<{ skipped: [] }>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()

const confirming = ref(false)
const confirm = ref('')

const skip = useMutation({
  mutationFn: async () => {
    if (props.imported) await setupResource.discard({ confirm: confirm.value })
    await setupResource.finish()
    // Before the wait: once the server leaves setup mode this component unmounts, and the page
    // must already know the wizard continues with the first site, not the cutover view.
    emit('skipped')
    await serverStarted()
  },
})
const error = computed(() => (skip.error.value instanceof Error ? skip.error.value : null))

function start(): void {
  if (props.imported) confirming.value = true
  else skip.mutate()
}
</script>

<template>
  <div class="grid gap-3.5">
    <Button
      v-if="!confirming"
      variant="ghost"
      class="justify-self-start"
      :loading="skip.isPending.value"
      @click="start"
    >
      {{ skip.isPending.value ? t('setup.admin.starting') : t('setup.skip.action') }}
    </Button>
    <form
      v-else
      class="grid gap-3.5 rounded-[var(--radius-control)] bg-fill p-4"
      novalidate
      @submit.prevent="skip.mutate()"
    >
      <p class="text-sm text-muted">
        {{ t('setup.skip.hint', { name: runtimeConfig.instance_name }) }}
      </p>
      <FormField
        id="setup-skip-confirm"
        v-model="confirm"
        autocomplete="off"
        :label="t('setup.report.discardLabel')"
        :error="fieldMessage(error, 'confirm')"
      />
      <CommandAlert
        :shown="['confirm']"
        :error="error"
      />
      <div class="flex gap-2">
        <Button
          type="submit"
          variant="destructive"
          :loading="skip.isPending.value"
          :disabled="confirm.trim() !== runtimeConfig.instance_name.trim()"
        >
          {{ t('setup.skip.confirm') }}
        </Button>
        <Button
          type="button"
          variant="ghost"
          @click="confirming = false"
        >
          {{ t('actions.cancel') }}
        </Button>
      </div>
    </form>
    <Alert
      v-if="error && !confirming"
      color="destructive"
    >
      <p class="text-sm">{{ message(error) }}</p>
    </Alert>
  </div>
</template>
