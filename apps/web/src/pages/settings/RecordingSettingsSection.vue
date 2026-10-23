<script setup lang="ts">
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Alert } from '~/components/ui/alert'
import { EmptyState } from '~/components/ui/empty-state'
import { Progress } from '~/components/ui/progress'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import { useRecordingBudget, useRecordingPolicy } from '~/composables/useRecordings'
import type { Site } from '~/generated'
import RecordingPolicyForm from '~/pages/recordings/RecordingPolicyForm.vue'
import { useI18n } from '~/plugins/i18n'

/** Settings pages pass `revision` to every section; recording policy carries its own. */
const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const format = useRecordingFormat()

const policy = useRecordingPolicy(() => props.site.id)
const budget = useRecordingBudget(() => props.site.id)
const share = (used: number, cap: number) => (cap > 0 ? (used / cap) * 100 : 0)
</script>

<template>
  <SettingsPage>
    <div
      v-if="policy.isPending.value"
      class="pb-8"
      aria-busy="true"
      :aria-label="t('states.loading')"
    >
      <Skeleton class="h-96 w-full" />
    </div>
    <EmptyState
      v-else-if="policy.isError.value"
      class="pb-8"
      icon="lucide:shield-alert"
      :tone="site.capabilities.includes('site.configure') ? 'error' : 'empty'"
      :title="message(policy.error.value)"
    />
    <RecordingPolicyForm
      v-else-if="policy.data.value"
      :key="site.id"
      :site="site.id"
      :policy="policy.data.value.policy"
      :revision="policy.data.value.revision"
      :can-configure="site.capabilities.includes('site.configure')"
    />

    <SettingsSection
      v-if="budget.data.value"
      id="recording-storage"
      :title="t('recording.budget.title')"
      :description="t('recording.budget.description')"
    >
      <Alert
        v-if="budget.data.value.refusing"
        color="warning"
        icon="lucide:hard-drive"
      >
        <p class="text-sm">{{ t('recording.budget.refusing') }}</p>
      </Alert>
      <div class="grid gap-2">
        <p class="text-sm text-primary tabular-nums">
          {{
            t('recording.budget.used', {
              used: format.bytes(budget.data.value.used_bytes),
              cap: format.bytes(budget.data.value.cap_bytes),
              captures: budget.data.value.captures,
            })
          }}
        </p>
        <Progress
          :model-value="share(budget.data.value.used_bytes, budget.data.value.cap_bytes)"
          :variant="budget.data.value.refusing ? 'destructive' : 'default'"
          :aria-label="t('recording.budget.title')"
        />
      </div>
      <div class="grid gap-2">
        <p class="text-sm text-primary tabular-nums">
          {{
            t('recording.budget.saved', {
              used: format.bytes(budget.data.value.saved_bytes),
              cap: format.bytes(budget.data.value.saved_cap_bytes),
            })
          }}
        </p>
        <Progress
          :model-value="share(budget.data.value.saved_bytes, budget.data.value.saved_cap_bytes)"
          :aria-label="t('replays.saved')"
        />
      </div>
      <p class="text-xs text-muted">{{ t('recording.budget.retention') }}</p>
    </SettingsSection>
  </SettingsPage>
</template>
