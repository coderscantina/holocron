<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import GeoipSection from '~/components/admin/GeoipSection.vue'
import RetentionDefaultSection from '~/components/admin/RetentionDefaultSection.vue'
import { Label } from '~/components/ui/label'
import { Skeleton } from '~/components/ui/skeleton'
import { Switch } from '~/components/ui/switch'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import type { PolicyTarget, SecurityUpdatePolicyArgs } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Where a second factor is required. A requirement applies at the next request: people without
 * a factor can then only enroll (decision 0007). User requirements are set per account on the
 * users page.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const reauth = useReauth()

const policy = useQuery({ queryKey: queryKeys.securityPolicy, queryFn: ({ signal }) => adminResource.securityPolicy(signal) })
const busy = ref<string | null>(null)

const set = async (target: PolicyTarget, required: boolean, key: string) => {
  const args: SecurityUpdatePolicyArgs = { target, two_factor_required: required }
  busy.value = key
  try {
    const result = await reauth.run('security.updatePolicy', args, () => adminResource.updateSecurityPolicy(args))
    if (!result) return
    queryClient.setQueryData(queryKeys.securityPolicy, (current: typeof policy.data.value) =>
      current ? { ...current, ...result } : current
    )
    toast.success(t('admin.security.saved'))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = null
  }
}
</script>

<template>
  <Skeleton
    v-if="policy.isPending.value"
    class="h-48 w-full max-w-5xl"
  />
  <p
    v-else-if="policy.isError.value"
    class="text-sm text-destructive"
  >
    {{ message(policy.error.value) }}
  </p>
  <SettingsPage
    v-else-if="policy.data.value"
    class="max-w-5xl"
  >
    <SettingsSection
      id="installation-policy"
      :title="t('admin.security.installation')"
      :description="t('admin.security.installationDescription')"
    >
      <div class="flex items-center gap-3">
        <Switch
          id="installation-two-factor"
          :model-value="policy.data.value.installation_two_factor_required"
          :disabled="!policy.data.value.can_manage_installation || busy !== null"
          @update:model-value="(value: boolean) => set({ scope: 'installation' }, value, 'installation')"
        />
        <Label for="installation-two-factor">{{ t('admin.security.everyone') }}</Label>
      </div>
    </SettingsSection>
    <SettingsSection
      id="team-policies"
      :title="t('admin.security.teams')"
      :description="t('admin.security.teamsDescription')"
    >
      <p
        v-if="policy.data.value.teams.length === 0"
        class="text-sm text-muted"
      >
        {{ t('admin.security.noTeams') }}
      </p>
      <ul
        v-else
        class="divide-y divide-divider rounded-panel border border-hairline bg-card"
      >
        <li
          v-for="team in policy.data.value.teams"
          :key="team.workspace"
          class="flex h-table-row items-center justify-between gap-3 px-4"
        >
          <Label
            :for="`team-policy-${team.workspace}`"
            class="min-w-0 truncate"
          >
            {{ team.name }}
          </Label>
          <Switch
            :id="`team-policy-${team.workspace}`"
            :model-value="team.two_factor_required"
            :disabled="busy !== null"
            @update:model-value="
              (value: boolean) => set({ scope: 'team', workspace: team.workspace }, value, team.workspace)
            "
          />
        </li>
      </ul>
    </SettingsSection>
    <RetentionDefaultSection />
    <GeoipSection />
  </SettingsPage>
</template>
