<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { ApiError } from '~/api'
import { sessionResource } from '~/api/resources/session'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { applyLoginResult } from '~/composables/useSession'
import type { SecondFactorProof } from '~/generated'
import { LOGIN_CHALLENGE_ATTEMPTS } from '~/generated/limits'
import { safeRedirect } from '~/lib/redirect'
import { useI18n } from '~/plugins/i18n'

/**
 * The second step of a login. The challenge is a cookie from the password step, valid for five
 * minutes and a few wrong codes; once it is gone the person starts over with the password.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const verify = useMutation({ mutationFn: (proof: SecondFactorProof) => sessionResource.verifySecondFactor(proof) })

usePageMeta(() => t('twoFactor.challengeTitle'))

const code = ref('')
const recovery = ref(false)

/** The challenge ended (expired, used up, or never started): only a new password step helps. */
const expired = computed(() => {
  const error = verify.error.value
  return error instanceof ApiError && (error.code === 'unauthenticated' || error.code === 'not_found')
})

const toggle = () => {
  recovery.value = !recovery.value
  code.value = ''
  verify.reset()
}

const submit = async () => {
  const proof: SecondFactorProof = { kind: recovery.value ? 'recovery_code' : 'totp', code: code.value.trim() }
  try {
    const next = applyLoginResult(await verify.mutateAsync(proof))
    const redirect = safeRedirect(route.query)
    if (next === 'enroll') await router.replace({ name: 'enroll', query: redirect === '/' ? {} : { redirect } })
    else if (next === 'signed_in') await router.replace(redirect)
  } catch {
    code.value = ''
  }
}
</script>

<template>
  <form
    class="grid gap-4"
    @submit.prevent="submit"
  >
    <div class="mb-3 grid gap-3">
      <h1 class="text-4xl font-semibold tracking-tight text-primary">{{ t('twoFactor.challengeTitle') }}</h1>
      <p class="text-sm text-muted">
        {{ recovery ? t('twoFactor.challengeRecovery') : t('twoFactor.challengeDescription') }}
      </p>
    </div>
    <FormField
      id="second-factor"
      v-model="code"
      :autocomplete="recovery ? 'off' : 'one-time-code'"
      :inputmode="recovery ? 'text' : 'numeric'"
      :label="recovery ? t('twoFactor.recoveryCode') : t('twoFactor.code')"
      :description="recovery ? t('twoFactor.recoveryHint') : undefined"
      required
    />
    <Alert
      v-if="expired"
      color="warning"
    >
      <p class="text-sm">{{ t('twoFactor.challengeExpired', { attempts: LOGIN_CHALLENGE_ATTEMPTS }) }}</p>
      <Button
        as-child
        variant="outline"
        size="sm"
        class="mt-2"
      >
        <RouterLink :to="{ name: 'login', query: route.query }">{{ t('twoFactor.startOver') }}</RouterLink>
      </Button>
    </Alert>
    <Alert
      v-else-if="verify.error.value"
      color="destructive"
    >
      <p class="text-sm">{{ message(verify.error.value) }}</p>
    </Alert>
    <Button
      type="submit"
      variant="accent"
      size="lg"
      :loading="verify.isPending.value"
      :disabled="code.trim() === ''"
    >
      {{ t('twoFactor.verify') }}
    </Button>
    <button
      type="button"
      class="w-fit cursor-pointer text-sm text-muted hover:text-primary hover:underline"
      @click="toggle"
    >
      {{ recovery ? t('twoFactor.useApp') : t('twoFactor.useRecoveryCode') }}
    </button>
  </form>
</template>
