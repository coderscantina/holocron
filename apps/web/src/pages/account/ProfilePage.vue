<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Skeleton } from '~/components/ui/skeleton'
import { useCommandForm } from '~/composables/useCommandForm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { patchSession } from '~/composables/useSession'
import type { AccountUpdateArgs, InstallRole } from '~/generated'
import { ACCOUNT_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

const { t, locale } = useI18n()
const { fieldMessage } = useErrorMessage()
usePageMeta(() => t('account.profile.title'))

const query = useQuery({ queryKey: queryKeys.account, queryFn: ({ signal }) => accountResource.get(signal) })
const update = useMutation({
  mutationFn: ({ args, key }: { args: AccountUpdateArgs; key: string }) => accountResource.update(args, key),
  onSuccess: (result) => {
    queryClient.setQueryData(queryKeys.account, (current: typeof query.data.value) =>
      current ? { ...current, account: result.account } : current
    )
    patchSession({ account: result.account })
    toast.success(t('account.profile.saved'))
  },
})

const name = ref('')
const email = ref('')
watch(
  () => query.data.value?.account,
  (account) => {
    if (!account) return
    name.value = account.name
    email.value = account.email
  },
  { immediate: true }
)

const dirty = computed(() => {
  const account = query.data.value?.account
  return !!account && (name.value !== account.name || email.value !== account.email)
})
const { fieldErrors, submit } = useCommandForm([name, email], () => update.error.value)
const save = () => submit((key) => update.mutateAsync({ args: { name: name.value, email: email.value }, key }))

const installRoles: Record<InstallRole, MessageKey> = {
  user: 'account.roles.user',
  install_admin: 'account.roles.install_admin',
}
const memberSince = computed(() => {
  const created = query.data.value?.created_at
  return created ? new Intl.DateTimeFormat(locale.value, { dateStyle: 'long' }).format(created) : ''
})
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="profile"
      :title="t('account.profile.identity')"
      :description="t('account.profile.identityDescription')"
    >
      <Skeleton
        v-if="query.isPending.value"
        class="h-40 w-full"
      />
      <form
        v-else
        id="profile-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save().catch(() => {})"
      >
        <FormField
          id="profile-name"
          v-model="name"
          autocomplete="name"
          :label="t('account.name')"
          :description="t('account.profile.nameHint', { max: ACCOUNT_NAME_MAX_CHARS })"
          :error="fieldErrors ? fieldMessage(update.error.value, 'name') : null"
          required
        />
        <FormField
          id="profile-email"
          v-model="email"
          type="email"
          autocomplete="email"
          :label="t('auth.email')"
          :error="fieldErrors ? fieldMessage(update.error.value, 'email') : null"
          required
        />
        <CommandAlert
          :shown="['name', 'email']"
          :error="update.error.value"
          @reload="query.refetch()"
        />
      </form>
      <template #footer>
        <Button
          type="submit"
          form="profile-form"
          variant="primary"
          :loading="update.isPending.value"
          :disabled="!dirty"
        >
          {{ t('actions.save') }}
        </Button>
      </template>
    </SettingsSection>
    <SettingsSection
      v-if="query.data.value"
      id="installation"
      :title="t('account.profile.installation')"
    >
      <dl class="grid gap-3 text-sm sm:grid-cols-[10rem_1fr]">
        <dt class="font-medium text-primary">{{ t('account.installRole') }}</dt>
        <dd>{{ t(installRoles[query.data.value.account.install_role]) }}</dd>
        <dt class="font-medium text-primary">{{ t('account.profile.memberSince') }}</dt>
        <dd>{{ memberSince }}</dd>
      </dl>
    </SettingsSection>
  </SettingsPage>
</template>
