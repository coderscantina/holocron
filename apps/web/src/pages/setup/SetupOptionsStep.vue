<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { RadioGroupItem, RadioGroupRoot } from 'reka-ui'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import type { Retention, SetupImportPreview, SetupImportStatus } from '~/generated'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

/**
 * The import options before the run. A resume keeps the options the run started with, so only
 * the 2FA key, which is never stored, can be given again.
 */
const props = defineProps<{ source: string; preview: SetupImportPreview; status: SetupImportStatus }>()
const emit = defineEmits<{ started: [] }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const { integer } = useFormat()

const stored = props.status.options
const resuming = stored !== undefined
const timezones = Intl.supportedValuesOf('timeZone')
const timezone = ref(stored?.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone)
const retention = ref<Retention>(stored?.retention ?? 'forever')
const loginDomain = ref(stored?.login_domain ?? 'umami.invalid')
const chunkRows = ref(String(stored?.chunk_rows ?? 5000))
/** Held in memory only, like the source URL. */
const twoFactorKey = ref('')

const start = useMutation({
  mutationFn: () =>
    setupResource.run({
      source: props.source.trim(),
      options: {
        timezone: timezone.value,
        retention: retention.value,
        login_domain: loginDomain.value.trim(),
        chunk_rows: Number(chunkRows.value),
      },
      two_factor_key: twoFactorKey.value.trim() || undefined,
    }),
  onSuccess: () => emit('started'),
})
const error = computed(() => (start.error.value instanceof Error ? start.error.value : null))
</script>

<template>
  <form
    class="grid gap-7"
    data-wide
    novalidate
    @submit.prevent="start.mutate()"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.options.title') }}
      <span class="block font-medium text-muted">
        {{ resuming ? t('setup.options.resumeSubtitle') : t('setup.options.subtitle') }}
      </span>
    </h1>
    <div class="grid gap-5">
      <div class="grid gap-1.5">
        <Label for="setup-import-timezone">{{ t('setup.options.timezone') }}</Label>
        <Select
          v-model="timezone"
          :disabled="resuming"
        >
          <SelectTrigger id="setup-import-timezone">
            <SelectValue />
          </SelectTrigger>
          <SelectContent class="max-h-80">
            <SelectItem
              v-for="zone in timezones"
              :key="zone"
              :value="zone"
            >
              {{ zone }}
            </SelectItem>
          </SelectContent>
        </Select>
        <p class="text-xs text-muted">{{ t('setup.options.timezoneHint') }}</p>
      </div>
      <div class="grid gap-1.5">
        <span
          id="setup-import-retention"
          class="text-sm font-medium text-primary"
        >
          {{ t('setup.options.retention') }}
        </span>
        <RadioGroupRoot
          v-model="retention"
          class="grid gap-1.5 sm:grid-cols-2"
          aria-labelledby="setup-import-retention"
          :disabled="resuming"
        >
          <RadioGroupItem
            v-for="choice in preview.retention"
            :key="choice.retention"
            :value="choice.retention"
            class="grid cursor-pointer gap-0.5 rounded-[var(--radius-control)] bg-fill px-3.5 py-2.5 text-left transition-colors duration-200 ease-butter focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-60 data-[state=checked]:bg-surface data-[state=checked]:inset-ring data-[state=checked]:inset-ring-accent"
          >
            <span class="text-sm font-medium text-primary">{{ t(`dataRetention.values.${choice.retention}`) }}</span>
            <span class="text-xs text-muted">
              {{
                choice.events_outside > 0
                  ? t('setup.options.leaves', { count: integer(choice.events_outside) })
                  : t('setup.options.keepsAll')
              }}
            </span>
          </RadioGroupItem>
        </RadioGroupRoot>
      </div>
      <FormField
        id="setup-import-two-factor-key"
        v-model="twoFactorKey"
        type="password"
        autocomplete="off"
        :label="t('setup.options.twoFactorKey')"
        :description="t('setup.options.twoFactorKeyHint')"
        :error="fieldMessage(error, 'two_factor_key')"
      />
      <details class="group rounded-[var(--radius-control)] bg-fill px-4 py-3">
        <summary class="cursor-pointer text-sm font-medium text-primary">{{ t('setup.options.advanced') }}</summary>
        <div class="mt-3.5 grid gap-3.5">
          <FormField
            id="setup-import-login-domain"
            v-model="loginDomain"
            autocomplete="off"
            :readonly="resuming"
            :label="t('setup.options.loginDomain')"
            :description="t('setup.options.loginDomainHint', { domain: loginDomain.trim() || 'umami.invalid' })"
            :error="fieldMessage(error, 'login_domain')"
          />
          <FormField
            id="setup-import-chunk-rows"
            v-model="chunkRows"
            inputmode="numeric"
            autocomplete="off"
            :readonly="resuming"
            :label="t('setup.options.chunkRows')"
            :description="t('setup.options.chunkRowsHint')"
            :error="fieldMessage(error, 'chunk_rows')"
          />
        </div>
      </details>
      <CommandAlert
        :shown="['two_factor_key', 'login_domain', 'chunk_rows']"
        :error="error"
      />
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="start.isPending.value"
      >
        {{ resuming ? t('setup.options.resume') : t('setup.options.start') }}
      </Button>
    </div>
  </form>
</template>
