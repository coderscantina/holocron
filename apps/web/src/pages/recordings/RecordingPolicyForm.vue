<script setup lang="ts">
import { ApiError } from '~/api'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Switch } from '~/components/ui/switch'
import { Textarea } from '~/components/ui/textarea'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useRecordingPolicy, useUpdateRecordingPolicy } from '~/composables/useRecordings'
import type { RecordingPolicy } from '~/generated'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ site: string; policy: RecordingPolicy; revision: number; canConfigure: boolean }>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const update = useUpdateRecordingPolicy()
const policyQuery = useRecordingPolicy(() => props.site)

const lines = (values: string[]) => values.join('\n')
const fromLines = (text: string) => text.split('\n').map((line) => line.trim()).filter(Boolean)

const replay = ref(props.policy.replay_enabled)
const heatmaps = ref(props.policy.heatmap_enabled)
const replaySample = ref(String(props.policy.replay_sample_percent))
const heatmapSample = ref(String(props.policy.heatmap_sample_percent))
const maxDuration = ref(String(props.policy.max_duration_minutes))
const elements = ref(lines(props.policy.excluded_elements))
const paths = ref(lines(props.policy.excluded_paths))
const saved = ref(false)

/** One idempotency key per intent, as in the site settings form. */
const intentKey = ref(crypto.randomUUID())
watch([replay, heatmaps, replaySample, heatmapSample, maxDuration, elements, paths], () => {
  intentKey.value = crypto.randomUUID()
  saved.value = false
})
watch(
  () => props.policy,
  (policy) => {
    replay.value = policy.replay_enabled
    heatmaps.value = policy.heatmap_enabled
    replaySample.value = String(policy.replay_sample_percent)
    heatmapSample.value = String(policy.heatmap_sample_percent)
    maxDuration.value = String(policy.max_duration_minutes)
    elements.value = lines(policy.excluded_elements)
    paths.value = lines(policy.excluded_paths)
  }
)

const error = computed(() => update.error.value)
const stale = computed(() => error.value instanceof ApiError && error.value.code === 'stale_revision')
const fieldErrors = computed(() => error.value instanceof ApiError && error.value.code === 'invalid_arguments')
const fieldError = (field: string) => (fieldErrors.value ? fieldMessage(error.value, field) : null)

const submit = async () => {
  saved.value = false
  await update.mutateAsync({
    args: {
      site: props.site,
      replay_enabled: replay.value,
      heatmap_enabled: heatmaps.value,
      replay_sample_percent: Number(replaySample.value),
      heatmap_sample_percent: Number(heatmapSample.value),
      max_duration_minutes: Number(maxDuration.value),
      excluded_elements: fromLines(elements.value),
      excluded_paths: fromLines(paths.value),
      expected_revision: props.revision,
    },
    idempotencyKey: intentKey.value,
  })
  saved.value = true
}

const reload = async () => {
  update.reset()
  await policyQuery.refetch()
}
</script>

<template>
  <SettingsSection
    id="recording-policy"
    :title="t('recording.title')"
    :description="canConfigure ? t('recording.description') : t('settings.readOnly')"
  >
    <form
      class="grid gap-5"
      novalidate
      @submit.prevent="submit().catch(() => {})"
    >
      <Alert
        color="info"
        icon="lucide:eye-off"
      >
        <p class="text-sm">{{ t('recording.masking') }}</p>
        <p class="mt-1 text-xs">{{ t('recording.maskingLimit') }}</p>
      </Alert>

      <div class="grid gap-4 sm:grid-cols-2">
        <div class="flex items-start gap-3">
          <Switch
            id="recording-replay"
            v-model="replay"
            :disabled="!canConfigure"
            aria-describedby="recording-replay-hint"
          />
          <div class="grid gap-0.5">
            <Label for="recording-replay">{{ t('recording.replay') }}</Label>
            <p
              id="recording-replay-hint"
              class="text-xs text-muted"
            >
              {{ t('recording.replayHint') }}
            </p>
          </div>
        </div>
        <div class="flex items-start gap-3">
          <Switch
            id="recording-heatmaps"
            v-model="heatmaps"
            :disabled="!canConfigure"
            aria-describedby="recording-heatmaps-hint"
          />
          <div class="grid gap-0.5">
            <Label for="recording-heatmaps">{{ t('recording.heatmaps') }}</Label>
            <p
              id="recording-heatmaps-hint"
              class="text-xs text-muted"
            >
              {{ t('recording.heatmapsHint') }}
            </p>
          </div>
        </div>
      </div>

      <div class="grid gap-4 sm:grid-cols-3">
        <FormField
          id="recording-replay-sample"
          v-model="replaySample"
          type="number"
          :label="t('recording.replaySample')"
          :description="t('recording.sampleHint')"
          :error="fieldError('replay_sample_percent')"
          :readonly="!canConfigure"
        />
        <FormField
          id="recording-heatmap-sample"
          v-model="heatmapSample"
          type="number"
          :label="t('recording.heatmapSample')"
          :description="t('recording.sampleHint')"
          :error="fieldError('heatmap_sample_percent')"
          :readonly="!canConfigure"
        />
        <FormField
          id="recording-duration"
          v-model="maxDuration"
          type="number"
          :label="t('recording.maxDuration')"
          :description="t('recording.maxDurationHint')"
          :error="fieldError('max_duration_minutes')"
          :readonly="!canConfigure"
        />
      </div>

      <div class="grid gap-1.5">
        <Label for="recording-elements">{{ t('recording.excludedElements') }}</Label>
        <Textarea
          id="recording-elements"
          v-model="elements"
          class="font-mono"
          :readonly="!canConfigure"
          :aria-invalid="fieldError('excluded_elements') ? 'true' : undefined"
          aria-describedby="recording-elements-hint recording-elements-error"
          spellcheck="false"
        />
        <p
          id="recording-elements-hint"
          class="text-xs text-muted"
        >
          {{ t('recording.excludedElementsHint') }}
        </p>
        <p
          v-if="fieldError('excluded_elements')"
          id="recording-elements-error"
          class="text-xs font-medium text-destructive"
        >
          {{ fieldError('excluded_elements') }}
        </p>
      </div>

      <div class="grid gap-1.5">
        <Label for="recording-paths">{{ t('recording.excludedPaths') }}</Label>
        <Textarea
          id="recording-paths"
          v-model="paths"
          class="font-mono"
          :readonly="!canConfigure"
          :aria-invalid="fieldError('excluded_paths') ? 'true' : undefined"
          aria-describedby="recording-paths-hint recording-paths-error"
          spellcheck="false"
        />
        <p
          id="recording-paths-hint"
          class="text-xs text-muted"
        >
          {{ t('recording.excludedPathsHint') }}
        </p>
        <p
          v-if="fieldError('excluded_paths')"
          id="recording-paths-error"
          class="text-xs font-medium text-destructive"
        >
          {{ fieldError('excluded_paths') }}
        </p>
      </div>

      <Alert
        v-if="stale"
        color="warning"
      >
        <p class="text-sm">{{ t('errors.stale_revision') }}</p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          class="mt-2"
          @click="reload()"
        >
          {{ t('settings.reload') }}
        </Button>
      </Alert>
      <Alert
        v-else-if="error && !fieldErrors"
        color="destructive"
      >
        <p class="text-sm">{{ message(error) }}</p>
      </Alert>

      <div
        v-if="canConfigure"
        class="flex flex-wrap items-center gap-3"
      >
        <Button
          type="submit"
          variant="accent"
          :loading="update.isPending.value"
        >
          <Icon
            name="lucide:save"
            aria-hidden="true"
          />
          {{ t('settings.save') }}
        </Button>
        <p
          class="text-sm text-muted"
          role="status"
        >
          {{ saved ? t('settings.saved') : '' }}
        </p>
      </div>
    </form>
  </SettingsSection>
</template>
