<script setup lang="ts">
import TwoFactorSetup from '~/components/account/TwoFactorSetup.vue'
import { Button } from '~/components/ui/button'
import { usePageMeta } from '~/composables/usePageMeta'
import { patchSession, useSession, useSignOut } from '~/composables/useSession'
import { safeRedirect } from '~/lib/redirect'
import { useI18n } from '~/plugins/i18n'

/**
 * A 2FA requirement applies and the account has no second factor yet. The session may only
 * enroll (decision 0007); confirming upgrades the same session, so no new sign-in is needed.
 */
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const { account } = useSession()
const signOut = useSignOut()

usePageMeta(() => t('twoFactor.enroll.title'))

const finish = () => router.replace(safeRedirect(route.query))
</script>

<template>
  <div class="grid gap-5">
    <div class="mb-2 grid gap-3">
      <h1 class="text-4xl font-semibold tracking-tight text-primary">{{ t('twoFactor.enroll.title') }}</h1>
      <p class="text-sm text-muted">{{ t('twoFactor.enroll.description', { email: account?.email ?? '' }) }}</p>
    </div>
    <TwoFactorSetup
      @enrolled="patchSession({ enrollment_only: false })"
      @done="finish"
    />
    <Button
      variant="link"
      class="justify-self-start text-muted"
      @click="signOut()"
    >
      {{ t('auth.signOut') }}
    </Button>
  </div>
</template>
