<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import SecretReveal from '~/components/account/SecretReveal.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Switch } from '~/components/ui/switch'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import type { InstallRole, SecurityUpdatePolicyArgs, UserCreated, UserSummary } from '~/generated'
import { ACCOUNT_NAME_MAX_CHARS } from '~/generated/limits'
import { installRoleLabels, installRoles } from '~/lib/roles'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Creates an account or edits one. A new account gets a generated password, shown once, which
 * the admin passes on; there is no self sign-up (decision 0012).
 */
const props = defineProps<{ user: UserSummary | null }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const reauth = useReauth()

const name = ref('')
const email = ref('')
const role = ref<InstallRole>('user')
const required = ref(false)
const created = ref<UserCreated | null>(null)

watch(open, (value) => {
  if (!value) {
    created.value = null
    save.reset()
    return
  }
  name.value = props.user?.name ?? ''
  email.value = props.user?.email ?? ''
  role.value = props.user?.install_role ?? 'user'
  required.value = props.user?.two_factor_required ?? false
})

const save = useMutation({
  mutationFn: async () => {
    const user = props.user
    if (!user) {
      created.value = await adminResource.createUser({ name: name.value, email: email.value, install_role: role.value })
      return
    }
    await adminResource.updateUser({ user: user.id, name: name.value, email: email.value, install_role: role.value })
    if (required.value !== user.two_factor_required) {
      const args: SecurityUpdatePolicyArgs = { target: { scope: 'user', user: user.id }, two_factor_required: required.value }
      await reauth.run('security.updatePolicy', args, () => adminResource.updateSecurityPolicy(args))
    }
    open.value = false
  },
  onSettled: () => queryClient.invalidateQueries({ queryKey: queryKeys.users }),
})

const error = computed(() => (save.error.value instanceof Error ? save.error.value : null))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-md"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('admin.users.createdTitle')"
          :description="t('admin.users.createdDescription', { email: created.user.email })"
        />
        <SecretReveal
          id="user-password"
          :label="t('auth.password')"
          :secret="created.secret"
        />
        <div class="flex justify-end">
          <Button
            variant="primary"
            @click="open = false"
          >
            {{ t('actions.done') }}
          </Button>
        </div>
      </template>
      <template v-else>
        <DialogHeaderCombined
          :title="user ? t('admin.users.edit') : t('admin.users.create')"
          :description="user ? user.email : t('admin.users.createDescription')"
        />
        <form
          class="grid gap-4"
          novalidate
          @submit.prevent="save.mutate()"
        >
          <FormField
            id="user-name"
            v-model="name"
            autocomplete="off"
            :label="t('account.name')"
            :description="t('account.profile.nameHint', { max: ACCOUNT_NAME_MAX_CHARS })"
            :error="fieldMessage(error, 'name')"
            required
          />
          <FormField
            id="user-email"
            v-model="email"
            type="email"
            autocomplete="off"
            :label="t('auth.email')"
            :error="fieldMessage(error, 'email')"
            required
          />
          <div class="grid gap-1.5">
            <Label for="user-role">{{ t('account.installRole') }}</Label>
            <Select v-model="role">
              <SelectTrigger id="user-role">
                <SelectValue>{{ t(installRoleLabels[role]) }}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="entry in installRoles"
                  :key="entry"
                  :value="entry"
                >
                  {{ t(installRoleLabels[entry]) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div
            v-if="user"
            class="flex items-center gap-3"
          >
            <Switch
              id="user-two-factor"
              v-model="required"
            />
            <Label for="user-two-factor">{{ t('admin.users.twoFactorRequired') }}</Label>
          </div>
          <CommandAlert
          :shown="['name', 'email']" :error="error" />
          <div class="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              @click="open = false"
            >
              {{ t('actions.cancel') }}
            </Button>
            <Button
              type="submit"
              variant="primary"
              :loading="save.isPending.value"
              :disabled="!name.trim() || !email.trim()"
            >
              {{ user ? t('actions.save') : t('admin.users.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
