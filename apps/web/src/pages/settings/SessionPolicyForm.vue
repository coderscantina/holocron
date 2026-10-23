<script setup lang="ts">
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { useCommandForm } from '~/composables/useCommandForm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useSite } from '~/composables/useSiteContext'
import { useUpdateSessionPolicy } from '~/composables/useSitePolicies'
import type { Site } from '~/generated'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import RecalculationCard from '~/pages/settings/RecalculationCard.vue'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const update = useUpdateSessionPolicy()
const siteQuery = useSite(() => props.site.id)

const timezone = ref(props.site.timezone)
const windowMinutes = ref(String(props.site.session_window_minutes))

const canConfigure = computed(() => props.site.capabilities.includes('site.configure'))
const dirty = computed(
  () => timezone.value !== props.site.timezone || windowMinutes.value !== String(props.site.session_window_minutes)
)

/** The browser's IANA zones, plus the current one, which some browsers leave out (`UTC`). */
const timezones = computed(() => {
  const zones = new Set(Intl.supportedValuesOf('timeZone'))
  zones.add(props.site.timezone)
  return [...zones].sort()
})

const error = computed(() => update.error.value)
const { saved, fieldErrors, submit } = useCommandForm([timezone, windowMinutes], () => error.value)
const timezoneError = computed(() => (fieldErrors.value ? fieldMessage(error.value, 'timezone') : null))

watch(
  () => props.site,
  (site) => {
    timezone.value = site.timezone
    windowMinutes.value = String(site.session_window_minutes)
  }
)

const save = () =>
  submit((idempotencyKey) =>
    update.mutateAsync({
      args: {
        site: props.site.id,
        timezone: timezone.value,
        window_minutes: Number(windowMinutes.value),
        expected_revision: props.revision,
      },
      idempotencyKey,
    })
  )

const reload = async () => {
  update.reset()
  await siteQuery.refetch()
}
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="session-policy"
      :title="t('settings.sessions.title')"
      :description="canConfigure ? t('settings.sessions.description') : t('settings.readOnly')"
    >
      <template #aside>
        <Badge
          variant="surface"
          size="sm"
          class="mt-1 justify-self-start"
        >
          {{ t('settings.policyVersion', { version: site.session_policy_version }) }}
        </Badge>
      </template>
      <form
        id="session-policy-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save().catch(() => {})"
      >
        <FormField
          id="session-window"
          v-model="windowMinutes"
          type="number"
          :label="t('settings.sessions.window')"
          :description="t('settings.sessions.windowHint')"
          :error="fieldErrors ? fieldMessage(error, 'window_minutes') : null"
          :readonly="!canConfigure"
          required
        />
        <div class="grid gap-1.5">
          <Label for="session-timezone">{{ t('settings.timezone') }}</Label>
          <select
            id="session-timezone"
            v-model="timezone"
            :disabled="!canConfigure"
            :aria-invalid="timezoneError ? 'true' : undefined"
            :aria-describedby="timezoneError ? 'session-timezone-description session-timezone-error' : 'session-timezone-description'"
            class="h-9 w-full rounded-md border border-input-border bg-input px-3 text-sm text-primary shadow-sm focus-visible:border-ring focus-visible:outline-none disabled:text-muted aria-invalid:border-destructive"
          >
            <option
              v-for="zone in timezones"
              :key="zone"
              :value="zone"
            >
              {{ zone }}
            </option>
          </select>
          <p
            id="session-timezone-description"
            class="text-xs text-muted"
          >
            {{ t('settings.sessions.timezoneHint') }}
          </p>
          <p
            v-if="timezoneError"
            id="session-timezone-error"
            class="text-xs font-medium text-destructive"
          >
            {{ timezoneError }}
          </p>
        </div>

        <CommandAlert
          :error="error"
          @reload="reload()"
        />
      </form>
      <template
        v-if="canConfigure"
        #footer
      >
        <Button
          type="submit"
          form="session-policy-form"
          variant="accent"
          :loading="update.isPending.value"
          :disabled="!dirty && !update.isError.value"
        >
          {{ t('settings.save') }}
        </Button>
        <p
          class="text-sm text-muted"
          role="status"
        >
          {{ saved && !dirty ? t('settings.saved') : '' }}
        </p>
      </template>
    </SettingsSection>

    <RecalculationCard
      v-if="canConfigure"
      :site="site"
    />
  </SettingsPage>
</template>
