<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { SetupBackup } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** What the restore will use, and that an older backup is upgraded right after. */
const props = defineProps<{ token: string; backup: SetupBackup }>()
const emit = defineEmits<{ started: []; back: [] }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime } = useDateFormat()
const { bytes } = useRecordingFormat()

const start = useMutation({
  mutationFn: () => setupResource.restore({ token: props.token, path: props.backup.path }),
  onSuccess: (progress) => {
    if (progress.state !== 'failed') emit('started')
  },
})
</script>

<template>
  <form
    class="grid gap-7"
    @submit.prevent="start.mutate()"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.confirm.title') }}
      <span class="block font-medium text-muted">{{ t('setup.confirm.subtitle') }}</span>
    </h1>
    <dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 rounded-[var(--radius-control)] bg-fill p-4 text-sm">
      <dt class="text-muted">{{ t('setup.confirm.date') }}</dt>
      <dd class="text-primary">{{ dateTime(backup.created_at_ms) }}</dd>
      <dt class="text-muted">{{ t('setup.confirm.size') }}</dt>
      <dd class="text-primary tabular-nums">{{ bytes(backup.bytes) }}</dd>
      <dt class="text-muted">{{ t('setup.confirm.release') }}</dt>
      <dd class="text-primary">{{ backup.release }}</dd>
      <dt class="text-muted">{{ t('setup.confirm.path') }}</dt>
      <dd class="min-w-0 break-words font-mono text-xs text-primary">{{ backup.path }}</dd>
    </dl>
    <div class="grid gap-3.5">
      <Alert
        v-if="backup.upgrade"
        color="warning"
      >
        <p class="text-sm">{{ t('setup.confirm.upgrade', { release: backup.release }) }}</p>
      </Alert>
      <p class="text-sm text-muted">{{ t('setup.confirm.accounts') }}</p>
      <Alert
        v-if="start.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(start.error.value) }}</p>
      </Alert>
      <Alert
        v-if="start.data.value?.error"
        color="destructive"
      >
        <div class="grid gap-1 text-sm">
          <p class="font-medium">{{ t('setup.restore.failed') }}</p>
          <p class="break-words whitespace-pre-wrap">{{ start.data.value.error.message }}</p>
        </div>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="start.isPending.value"
      >
        {{ t('setup.confirm.start') }}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="lg"
        @click="emit('back')"
      >
        {{ t('setup.confirm.other') }}
      </Button>
    </div>
  </form>
</template>
