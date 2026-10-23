<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { adminResource } from '~/api/resources/admin'
import { teamsResource } from '~/api/resources/teams'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Switch } from '~/components/ui/switch'
import { useCommandForm } from '~/composables/useCommandForm'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { forgetWorkspace, refreshTeam } from '~/composables/useTeams'
import type { SecurityUpdatePolicyArgs, TeamDetail, TeamsUpdateArgs } from '~/generated'
import { ACCOUNT_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

/** A team's name, its 2FA requirement, and its deletion. Shown to people who manage the team. */
const props = defineProps<{ detail: TeamDetail; revision: number }>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const { confirm } = useConfirm()
const reauth = useReauth()
const router = useRouter()

const team = computed(() => props.detail.team)

const name = ref(team.value.name)
watch(
  () => team.value.name,
  (value) => {
    name.value = value
  }
)
const rename = useMutation({
  mutationFn: ({ args, key }: { args: TeamsUpdateArgs; key: string }) => teamsResource.update(args, key),
  onSuccess: () => {
    toast.success(t('workspace.saved'))
    return refreshTeam(team.value.id)
  },
})
const { fieldErrors, submit } = useCommandForm([name], () => rename.error.value)
const save = () =>
  submit((key) =>
    rename.mutateAsync({ args: { workspace: team.value.id, name: name.value, expected_revision: props.revision }, key })
  )

const policyBusy = ref(false)
const setRequirement = async (required: boolean) => {
  const args: SecurityUpdatePolicyArgs = {
    target: { scope: 'team', workspace: team.value.id },
    two_factor_required: required,
  }
  policyBusy.value = true
  try {
    const result = await reauth.run('security.updatePolicy', args, () => adminResource.updateSecurityPolicy(args))
    if (!result) return
    toast.success(required ? t('workspace.twoFactorOn') : t('workspace.twoFactorOff'))
    await refreshTeam(team.value.id)
  } catch (error) {
    toast.error(message(error))
  } finally {
    policyBusy.value = false
  }
}

const deleting = ref(false)
const remove = async () => {
  const ok = await confirm({
    title: t('workspace.delete'),
    message: t('workspace.deleteConfirm', { team: team.value.name, sites: team.value.sites }),
    confirmLabel: t('workspace.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  deleting.value = true
  const args = { workspace: team.value.id }
  try {
    const result = await reauth.run('teams.delete', args, () => teamsResource.remove(team.value.id))
    if (!result) return
    forgetWorkspace(team.value.id)
    toast.success(t('workspace.deleted', { team: team.value.name }))
    await router.push({ name: 'home' })
  } catch (error) {
    toast.error(message(error))
  } finally {
    deleting.value = false
  }
}
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="team-name"
      :title="t('workspace.teamName')"
    >
      <form
        id="team-name-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save().catch(() => {})"
      >
        <FormField
          id="team-name-input"
          v-model="name"
          autocomplete="organization"
          :label="t('workspace.teamName')"
          :description="t('account.profile.nameHint', { max: ACCOUNT_NAME_MAX_CHARS })"
          :error="fieldErrors ? fieldMessage(rename.error.value, 'name') : null"
          required
        />
        <CommandAlert
          :shown="['name']"
          :error="rename.error.value"
          @reload="refreshTeam(team.id)"
        />
      </form>
      <template #footer>
        <Button
          type="submit"
          form="team-name-form"
          variant="primary"
          :loading="rename.isPending.value"
          :disabled="name === team.name"
        >
          {{ t('actions.save') }}
        </Button>
      </template>
    </SettingsSection>
    <SettingsSection
      id="team-security"
      :title="t('workspace.twoFactor')"
      :description="t('workspace.twoFactorDescription')"
    >
      <div class="flex items-center gap-3">
        <Switch
          id="team-two-factor"
          :model-value="team.two_factor_required"
          :disabled="policyBusy"
          @update:model-value="setRequirement"
        />
        <Label for="team-two-factor">{{ t('workspace.twoFactorRequired') }}</Label>
      </div>
    </SettingsSection>
    <SettingsSection
      id="team-delete"
      :title="t('workspace.delete')"
      :description="t('workspace.deleteDescription')"
      destructive
    >
      <div>
        <Button
          variant="destructive"
          :loading="deleting"
          @click="remove"
        >
          {{ t('workspace.delete') }}
        </Button>
      </div>
    </SettingsSection>
  </SettingsPage>
</template>
