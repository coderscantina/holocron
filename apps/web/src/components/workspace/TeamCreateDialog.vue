<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { teamsResource } from '~/api/resources/teams'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { ACCOUNT_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/** A new team with the caller as its owner. Opens the team's page afterwards. */
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const router = useRouter()
const name = ref('')
let intent = crypto.randomUUID()

const create = useMutation({
  mutationFn: (value: string) => teamsResource.create(value, intent),
  onSuccess: async ({ team }) => {
    open.value = false
    await queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
    await router.push({ name: 'workspace', params: { workspaceId: team.id } })
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  intent = crypto.randomUUID()
  create.reset()
})
watch(name, () => {
  intent = crypto.randomUUID()
})

const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogHeaderCombined
        :title="t('workspace.createTeam')"
        :description="t('workspace.createTeamDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="create.mutate(name)"
      >
        <FormField
          id="team-name"
          v-model="name"
          autocomplete="organization"
          :label="t('workspace.teamName')"
          :description="t('account.profile.nameHint', { max: ACCOUNT_NAME_MAX_CHARS })"
          :error="fieldMessage(error, 'name')"
          required
        />
        <CommandAlert
          :shown="['name']" :error="error" />
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
            :loading="create.isPending.value"
            :disabled="name.trim() === ''"
          >
            {{ t('workspace.createTeam') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
