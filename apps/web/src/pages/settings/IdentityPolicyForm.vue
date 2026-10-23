<script setup lang="ts">
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useCommandForm } from '~/composables/useCommandForm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useIdentityPolicy, useUpdateIdentityPolicy } from '~/composables/useSitePolicies'
import type { Site } from '~/generated'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const canConfigure = computed(() => props.site.capabilities.includes('site.configure'))
const policy = useIdentityPolicy(() => props.site.id, canConfigure)
const update = useUpdateIdentityPolicy()

const enabled = ref(false)
watch(
  () => policy.data.value?.persistent_identity,
  (value) => {
    if (value !== undefined) enabled.value = value
  },
  { immediate: true }
)

const stored = computed(() => policy.data.value?.persistent_identity)
const dirty = computed(() => stored.value !== undefined && enabled.value !== stored.value)
const revoking = computed(() => dirty.value && !enabled.value)

const error = computed(() => update.error.value)
const { saved, submit } = useCommandForm([enabled], () => error.value)

const save = () =>
  submit((idempotencyKey) =>
    update.mutateAsync({
      args: { site: props.site.id, persistent_identity: enabled.value, expected_revision: props.revision },
      idempotencyKey,
    })
  )

const reload = async () => {
  update.reset()
  await policy.refetch()
}
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="identity-policy"
      :title="t('settings.identity.title')"
      :description="t('settings.identity.description')"
    >
      <template
        v-if="policy.data.value"
        #aside
      >
        <Badge
          variant="surface"
          size="sm"
          class="mt-1 justify-self-start"
        >
          {{ t('settings.policyVersion', { version: policy.data.value.identity_policy_version }) }}
        </Badge>
      </template>
      <form
        id="identity-policy-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save().catch(() => {})"
      >
        <p
          v-if="!canConfigure"
          class="text-sm text-muted"
        >
          {{ t('settings.identity.restricted') }}
        </p>
        <Skeleton
          v-else-if="policy.isPending.value"
          class="h-12 w-full"
        />
        <Alert
          v-else-if="policy.isError.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(policy.error.value) }}</p>
        </Alert>
        <template v-else>
          <div class="flex items-start gap-3">
            <input
              id="persistent-identity"
              v-model="enabled"
              type="checkbox"
              aria-describedby="persistent-identity-description"
              class="mt-0.5 size-4 shrink-0 accent-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
            <div class="grid gap-1">
              <label
                for="persistent-identity"
                class="text-sm font-medium text-primary"
              >
                {{ t('settings.identity.toggle') }}
              </label>
              <p
                id="persistent-identity-description"
                class="text-xs text-muted"
              >
                {{ t('settings.identity.toggleHint') }}
              </p>
            </div>
          </div>

          <Alert
            v-if="revoking"
            color="warning"
          >
            <p class="text-sm">{{ t('settings.identity.revokeWarning') }}</p>
          </Alert>

          <CommandAlert
            :error="error"
            @reload="reload()"
          />
        </template>
      </form>
      <template
        v-if="canConfigure && policy.data.value"
        #footer
      >
        <Button
          type="submit"
          form="identity-policy-form"
          :variant="revoking ? 'destructive' : 'accent'"
          :loading="update.isPending.value"
          :disabled="!dirty && !update.isError.value"
        >
          {{ revoking ? t('settings.identity.turnOff') : t('settings.save') }}
        </Button>
        <p
          class="text-sm text-muted"
          role="status"
        >
          {{ saved && !dirty ? t('settings.saved') : '' }}
        </p>
      </template>
    </SettingsSection>
  </SettingsPage>
</template>
