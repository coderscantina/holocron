<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Spinner } from '~/components/ui/spinner'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { SetupBackup } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** The backups in the server's backup directory, or any other path on the host. */
const props = defineProps<{ token: string }>()
const emit = defineEmits<{ picked: [backup: SetupBackup]; running: [] }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime } = useDateFormat()
const { bytes } = useRecordingFormat()

const listing = useQuery({
  queryKey: ['setup', 'backups'],
  queryFn: () => setupResource.backups({ token: props.token }),
  gcTime: 0,
  retry: false,
})
watch(
  () => listing.data.value?.restore.state,
  (state) => {
    if (state === 'running') emit('running')
  },
  { immediate: true }
)

const chosen = ref('')
const path = ref('')
const inspect = useMutation({
  mutationFn: (value: string) => setupResource.inspectBackup({ token: props.token, path: value }),
  onSuccess: (result) => {
    if (result.backup) emit('picked', result.backup)
  },
})

function submit(): void {
  const value = path.value.trim() || chosen.value
  if (value) inspect.mutate(value)
}
const ready = computed(() => Boolean(path.value.trim() || chosen.value))

onMounted(() => document.getElementById('setup-backup-path')?.focus())
</script>

<template>
  <form
    class="grid gap-7"
    data-wide
    novalidate
    @submit.prevent="submit"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.backup.title') }}
      <span class="block font-medium text-muted">{{ t('setup.backup.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <p
        v-if="listing.isPending.value"
        class="flex items-center gap-3 text-sm text-muted"
      >
        <Spinner class="size-4" />
        {{ t('setup.backup.loading') }}
      </p>
      <Alert
        v-else-if="listing.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(listing.error.value) }}</p>
      </Alert>
      <template v-else-if="listing.data.value">
        <RadioGroupRoot
          v-if="listing.data.value.backups.length > 0"
          v-model="chosen"
          class="grid gap-2"
          :aria-label="t('setup.backup.listLabel', { directory: listing.data.value.directory })"
          @update:model-value="path = ''"
        >
          <RadioGroupItem
            v-for="backup in listing.data.value.backups"
            :key="backup.path"
            :value="backup.path"
            class="group flex cursor-pointer items-start gap-3.5 rounded-[var(--radius-control)] bg-fill p-4 text-left transition-colors duration-200 ease-butter focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring data-[state=checked]:bg-surface data-[state=checked]:inset-ring data-[state=checked]:inset-ring-accent"
          >
            <span
              class="grid size-9 shrink-0 place-items-center rounded-[var(--radius-control)] bg-fill-strong text-foreground group-data-[state=checked]:bg-accent/15 group-data-[state=checked]:text-accent-text"
            >
              <Icon
                name="lucide:history"
                aria-hidden="true"
              />
            </span>
            <span class="grid min-w-0 gap-0.5">
              <span class="text-[15px] font-semibold text-primary">{{ dateTime(backup.created_at_ms) }}</span>
              <span class="text-sm text-muted">
                {{ t('setup.backup.details', { size: bytes(backup.bytes), release: backup.release }) }}
              </span>
              <span class="truncate font-mono text-xs text-muted">{{ backup.path }}</span>
            </span>
          </RadioGroupItem>
        </RadioGroupRoot>
        <p
          v-else
          class="rounded-[var(--radius-control)] bg-fill p-4 text-sm text-muted"
        >
          {{ t('setup.backup.none', { directory: listing.data.value.directory }) }}
        </p>
      </template>
      <FormField
        id="setup-backup-path"
        v-model="path"
        autocomplete="off"
        :label="t('setup.backup.path')"
        :description="t('setup.backup.pathHint')"
        @update:model-value="chosen = ''"
      />
      <Alert
        v-if="inspect.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(inspect.error.value) }}</p>
      </Alert>
      <Alert
        v-if="inspect.data.value?.refusal"
        color="destructive"
      >
        <div class="grid gap-1 text-sm">
          <p class="font-medium">{{ t('setup.backup.refused') }}</p>
          <p class="break-words whitespace-pre-wrap">{{ inspect.data.value.refusal.message }}</p>
        </div>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="inspect.isPending.value"
        :disabled="!ready"
      >
        {{ t('setup.continue') }}
      </Button>
    </div>
  </form>
</template>
