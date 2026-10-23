<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { SetupImportStatus, SetupInspection } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import { sourceLabel } from './source'

/** The Umami database URL and the inspect preview, or the refusal with its fix. */
const props = defineProps<{ status: SetupImportStatus }>()
const emit = defineEmits<{ inspected: [result: SetupInspection] }>()
const source = defineModel<string>({ required: true })

const { t } = useI18n()
const { message } = useErrorMessage()
const { date } = useDateFormat()
const { integer } = useFormat()
const { bytes } = useRecordingFormat()

const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
const inspect = useMutation({
  mutationFn: () => setupResource.inspect({ source: source.value.trim(), timezone }),
})
const preview = computed(() => inspect.data.value?.preview ?? null)
const refusal = computed(() => inspect.data.value?.refusal ?? null)

const resuming = computed(() => props.status.state === 'interrupted')

onMounted(() => document.getElementById('setup-source')?.focus())
</script>

<template>
  <form
    class="grid gap-7"
    data-wide
    novalidate
    @submit.prevent="inspect.mutate()"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.source.title') }}
      <span class="block font-medium text-muted">{{ t('setup.source.subtitle') }}</span>
    </h1>
    <Alert
      v-if="resuming"
      color="warning"
    >
      <div class="grid gap-1 text-sm">
        <p>{{ t('setup.source.resume', { source: sourceLabel(status.source ?? '') }) }}</p>
        <p
          v-if="status.error"
          class="text-muted"
        >
          {{ t('setup.source.stopped', { message: status.error.message }) }}
        </p>
      </div>
    </Alert>
    <Alert
      v-else-if="status.error"
      color="destructive"
    >
      <p class="text-sm break-words">{{ status.error.message }}</p>
    </Alert>
    <div class="grid gap-3.5">
      <FormField
        id="setup-source"
        v-model="source"
        autocomplete="off"
        :label="t('setup.source.label')"
        :description="t('setup.source.hint')"
        required
      />
      <Alert
        v-if="inspect.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(inspect.error.value) }}</p>
      </Alert>
      <Alert
        v-if="refusal"
        color="destructive"
      >
        <div class="grid gap-1 text-sm">
          <p class="font-medium">{{ t('setup.source.refused') }}</p>
          <p class="break-words whitespace-pre-wrap">{{ refusal.message }}</p>
        </div>
      </Alert>
      <Button
        v-if="!preview"
        type="submit"
        variant="accent"
        size="lg"
        :loading="inspect.isPending.value"
        :disabled="!source.trim()"
      >
        {{ inspect.isPending.value ? t('setup.source.inspecting') : t('setup.source.inspect') }}
      </Button>
    </div>
    <template v-if="preview && inspect.data.value">
      <Alert
        v-for="warning in preview.warnings"
        :key="warning.code"
        color="warning"
      >
        <div class="grid gap-1 text-sm">
          <p class="font-medium">{{ t(`setup.source.warnings.${warning.code}`) }}</p>
          <p class="break-words whitespace-pre-wrap">{{ warning.message }}</p>
        </div>
      </Alert>
      <ul class="grid gap-2">
        <li class="flex gap-3 rounded-[var(--radius-control)] bg-fill p-4">
          <Icon
            name="lucide:circle-check"
            class="mt-0.5 shrink-0 text-success"
            aria-hidden="true"
          />
          <div class="grid min-w-0 gap-0.5 text-sm">
            <p class="font-medium text-primary">{{ sourceLabel(preview.source) }}</p>
            <p class="text-muted">
              {{ t('setup.source.events', { count: integer(preview.events) }) }}
              <template v-if="preview.first_event !== undefined && preview.last_event !== undefined">
                · {{ t('setup.source.range', { first: date(preview.first_event), last: date(preview.last_event) }) }}
              </template>
            </p>
            <p class="text-muted">{{ t('setup.source.size', { size: bytes(preview.source_bytes) }) }}</p>
          </div>
        </li>
      </ul>
      <details class="group rounded-[var(--radius-control)] bg-fill px-4 py-3 text-sm">
        <summary class="cursor-pointer font-medium text-primary">{{ t('setup.source.tables') }}</summary>
        <dl class="mt-3 grid grid-cols-[1fr_auto] gap-x-6 gap-y-1">
          <template
            v-for="table in preview.tables"
            :key="table.table"
          >
            <dt class="truncate font-mono text-xs text-muted">{{ table.table }}</dt>
            <dd class="text-right tabular-nums text-primary">{{ integer(table.rows) }}</dd>
          </template>
        </dl>
      </details>
      <div class="flex flex-col gap-2 sm:flex-row">
        <Button
          type="button"
          variant="accent"
          size="lg"
          class="sm:flex-1"
          @click="emit('inspected', inspect.data.value)"
        >
          {{ t('setup.continue') }}
        </Button>
        <Button
          type="submit"
          variant="fill"
          size="lg"
          :loading="inspect.isPending.value"
        >
          {{ t('setup.source.again') }}
        </Button>
      </div>
    </template>
  </form>
</template>
