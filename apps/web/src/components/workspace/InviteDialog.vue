<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/people/InviteDialog.vue. Without an
// email service the dialog creates a single-use link and shows it once for the admin to pass on.
import { useMutation } from '@tanstack/vue-query'

import { teamsResource } from '~/api/resources/teams'
import SecretReveal from '~/components/account/SecretReveal.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { refreshTeam } from '~/composables/useTeams'
import type { InvitationCreated, Role, Team, TeamsInviteArgs } from '~/generated'
import { INVITATION_LIFETIME_MAX_DAYS, LABEL_MAX_CHARS } from '~/generated/limits'
import { assignableRoles, roleLabels } from '~/lib/roles'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ team: Team }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const DEFAULT_DAYS = 7
const role = ref<Role>('member')
const label = ref('')
const days = ref(String(DEFAULT_DAYS))
const created = ref<InvitationCreated | null>(null)

const invite = useMutation({
  mutationFn: (args: TeamsInviteArgs) => teamsResource.invite(args),
  onSuccess: (result) => {
    created.value = result
    void refreshTeam(props.team.id)
  },
})

watch(open, (value) => {
  if (value) return
  role.value = 'member'
  label.value = ''
  days.value = String(DEFAULT_DAYS)
  created.value = null
  invite.reset()
})

/** The join link carries the code; the app's own origin, since the admin passes it on as is. */
const link = computed(() =>
  created.value?.secret ? `${window.location.origin}/teams/join?code=${encodeURIComponent(created.value.secret)}` : null
)
const lifetime = computed(() => Number.parseInt(days.value, 10))
const valid = computed(() => Number.isInteger(lifetime.value) && lifetime.value >= 1 && lifetime.value <= INVITATION_LIFETIME_MAX_DAYS)
const error = computed(() => (invite.error.value instanceof Error ? invite.error.value : null))

const submit = () =>
  invite.mutate({ workspace: props.team.id, role: role.value, label: label.value, lifetime_days: lifetime.value })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-md"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('invitations.createdTitle')"
          :description="t('invitations.createdDescription')"
        />
        <SecretReveal
          id="invitation-link"
          :label="t('invitations.link')"
          :secret="link"
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
          :title="t('invitations.create')"
          :description="t('invitations.createDescription')"
        />
        <form
          class="grid gap-4"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="invite-label"
            v-model="label"
            autocomplete="off"
            :label="t('invitations.label')"
            :description="t('invitations.labelHint', { max: LABEL_MAX_CHARS })"
            :error="fieldMessage(error, 'label')"
          />
          <div class="grid gap-1.5">
            <Label for="invite-role">{{ t('members.role') }}</Label>
            <Select v-model="role">
              <SelectTrigger id="invite-role">
                <SelectValue>{{ t(roleLabels[role]) }}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="entry in assignableRoles(team.role)"
                  :key="entry"
                  :value="entry"
                >
                  {{ t(roleLabels[entry]) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <FormField
            id="invite-days"
            v-model="days"
            type="number"
            inputmode="numeric"
            :label="t('invitations.lifetime')"
            :description="t('invitations.lifetimeHint', { max: INVITATION_LIFETIME_MAX_DAYS })"
            :error="fieldMessage(error, 'lifetime_days')"
            required
          />
          <CommandAlert
          :shown="['label', 'lifetime_days']" :error="error" />
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
              :loading="invite.isPending.value"
              :disabled="!valid"
            >
              {{ t('invitations.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
