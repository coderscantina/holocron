<script setup lang="ts">
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { useCommandForm } from '~/composables/useCommandForm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useRecalculation, useStartRecalculation } from '~/composables/useSitePolicies'
import type { RecalculationId, Site } from '~/generated'
import { RECALCULATION_MAX_DAYS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const { fieldMessage, message } = useErrorMessage()
const start = useStartRecalculation()

/** `YYYY-MM-DD` in the site timezone, `daysAgo` days before today. */
const localDay = (daysAgo: number) =>
  new Intl.DateTimeFormat('en-CA', { timeZone: props.site.timezone }).format(Date.now() - daysAgo * 86_400_000)

const firstDay = ref(localDay(29))
const lastDay = ref(localDay(0))
const recalculation = ref<RecalculationId | null>(null)
const status = useRecalculation(() => props.site.id, recalculation)
const current = computed(() => status.data.value?.recalculation ?? null)

const error = computed(() => start.error.value)
const { fieldErrors, submit } = useCommandForm([firstDay, lastDay], () => error.value)

const run = () =>
  submit(async (idempotencyKey) => {
    const answer = await start.mutateAsync({
      args: { site: props.site.id, first_day: firstDay.value, last_day: lastDay.value },
      idempotencyKey,
    })
    recalculation.value = answer.recalculation.id
  })
</script>

<template>
  <SettingsSection
    id="recalculate"
    :title="t('settings.recalculate.title')"
    :description="t('settings.recalculate.description', { max: RECALCULATION_MAX_DAYS })"
  >
    <form
      id="recalculate-form"
      class="grid gap-4"
      novalidate
      @submit.prevent="run().catch(() => {})"
    >
      <div class="grid gap-4 sm:grid-cols-2">
        <FormField
          id="recalculate-first-day"
          v-model="firstDay"
          type="date"
          :label="t('settings.recalculate.firstDay')"
          :error="fieldErrors ? fieldMessage(error, 'first_day') : null"
          required
        />
        <FormField
          id="recalculate-last-day"
          v-model="lastDay"
          type="date"
          :label="t('settings.recalculate.lastDay')"
          :error="fieldErrors ? fieldMessage(error, 'last_day') : null"
          required
        />
      </div>

      <CommandAlert :error="error" />
    </form>
    <template #footer>
      <Button
        type="submit"
        form="recalculate-form"
        variant="fill"
        :loading="start.isPending.value"
        :disabled="current?.state === 'pending'"
      >
        {{ t('settings.recalculate.start') }}
      </Button>
      <p
        class="text-sm text-muted"
        role="status"
      >
        <template v-if="status.isError.value">{{ message(status.error.value) }}</template>
        <template v-else-if="current?.state === 'pending'">
          {{ t('settings.recalculate.pending', { first: current.first_day, last: current.last_day, days: current.pending_days }) }}
        </template>
        <template v-else-if="current">
          {{ t('settings.recalculate.done', { first: current.first_day, last: current.last_day }) }}
        </template>
      </p>
    </template>
  </SettingsSection>
</template>
