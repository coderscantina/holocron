<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { teamsResource } from '~/api/resources/teams'
import FormField from '~/components/FormField.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Card } from '~/components/ui/card'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSession } from '~/composables/useSession'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Redeems an invitation link. The person signs in first (the router guard sends them back
 * here); the code is used only when they confirm, so opening a link never joins by accident.
 * Unknown, used, revoked and expired codes all answer the same `not_found`.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()
const router = useRouter()
const { account } = useSession()
usePageMeta(() => t('join.title'))

const code = ref(typeof route.query.code === 'string' ? route.query.code : '')

const join = useMutation({
  mutationFn: (value: string) => teamsResource.join(value.trim()),
  onSuccess: async ({ team }) => {
    await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
    toast.success(t('join.joined', { team: team.name }))
    await router.replace({ name: 'workspace', params: { workspaceId: team.id } })
  },
})

const failure = computed(() => {
  const error = join.error.value
  if (!error) return null
  return error instanceof ApiError && error.code === 'not_found' ? t('join.invalid') : message(error)
})
</script>

<template>
  <Card class="mx-auto mt-8 grid max-w-md gap-4 p-6">
    <div>
      <h1 class="text-xl font-semibold text-primary">{{ t('join.title') }}</h1>
      <p class="text-sm text-muted">{{ t('join.description', { email: account?.email ?? '' }) }}</p>
    </div>
    <form
      class="grid gap-4"
      @submit.prevent="join.mutate(code)"
    >
      <FormField
        id="join-code"
        v-model="code"
        autocomplete="off"
        :label="t('join.code')"
        required
      />
      <Alert
        v-if="failure"
        color="destructive"
      >
        <p class="text-sm">{{ failure }}</p>
      </Alert>
      <Button
        type="submit"
        variant="accent"
        :loading="join.isPending.value"
        :disabled="code.trim() === ''"
      >
        {{ t('join.submit') }}
      </Button>
    </form>
  </Card>
</template>
