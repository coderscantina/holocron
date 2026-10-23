<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import SecretReveal from '~/components/account/SecretReveal.vue'
import { Button } from '~/components/ui/button'
import { Checkbox } from '~/components/ui/checkbox'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { useReauth } from '~/composables/useReauth'
import type { CredentialsReset, UserSummary, UsersResetCredentialsArgs } from '~/generated'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Account recovery without email: a generated password, removing the second factor, or both.
 * Ends every session of the person.
 */
const props = defineProps<{ user: UserSummary | null }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const reauth = useReauth()

const password = ref<boolean | 'indeterminate'>(true)
const twoFactor = ref<boolean | 'indeterminate'>(false)
const result = ref<CredentialsReset | null>(null)

watch(open, (value) => {
  if (value) return
  password.value = true
  twoFactor.value = false
  result.value = null
  reset.reset()
})

const reset = useMutation({
  mutationFn: async (args: UsersResetCredentialsArgs) =>
    reauth.run('users.resetCredentials', args, () => adminResource.resetCredentials(args)),
  onSuccess: (answer) => {
    if (!answer) return
    result.value = answer
    void queryClient.invalidateQueries({ queryKey: queryKeys.users })
  },
})

const submit = () => {
  if (!props.user) return
  reset.mutate({ user: props.user.id, password: password.value === true, two_factor: twoFactor.value === true })
}
const error = computed(() => (reset.error.value instanceof Error ? reset.error.value : null))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-md"
      @interact-outside="(event) => result && event.preventDefault()"
    >
      <template v-if="result">
        <DialogHeaderCombined
          :title="t('admin.users.resetDone')"
          :description="t('admin.users.resetDoneDescription', { email: result.user.email })"
        />
        <SecretReveal
          v-if="result.secret || result.secret_withheld"
          id="reset-password"
          :label="t('auth.password')"
          :secret="result.secret"
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
          :title="t('admin.users.reset')"
          :description="t('admin.users.resetDescription', { email: user?.email ?? '' })"
        />
        <form
          class="grid gap-4"
          @submit.prevent="submit"
        >
          <div class="flex items-center gap-2">
            <Checkbox
              id="reset-password-choice"
              v-model="password"
            />
            <Label for="reset-password-choice">{{ t('admin.users.resetPassword') }}</Label>
          </div>
          <div class="flex items-center gap-2">
            <Checkbox
              id="reset-two-factor-choice"
              v-model="twoFactor"
              :disabled="!user?.two_factor_enrolled"
            />
            <Label for="reset-two-factor-choice">{{ t('admin.users.resetTwoFactor') }}</Label>
          </div>
          <CommandAlert
          :shown="[]" :error="error" />
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
              variant="destructive"
              :loading="reset.isPending.value"
              :disabled="password !== true && twoFactor !== true"
            >
              {{ t('admin.users.reset') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
