<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/people/PeopleTable.vue. Members and
// invitations are separate tables, since invitations carry no account until they are used.
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { teamsResource } from '~/api/resources/teams'
import Icon from '~/components/Icon.vue'
import InviteDialog from '~/components/workspace/InviteDialog.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useSession } from '~/composables/useSession'
import { forgetWorkspace, refreshTeam } from '~/composables/useTeams'
import type { Invitation, Member, Role, TeamDetail } from '~/generated'
import { assignableRoles, invitationStateLabels, roleLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ detail: TeamDetail; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()
const reauth = useReauth()
const router = useRouter()
const { account } = useSession()

const team = computed(() => props.detail.team)
const manage = computed(() => team.value.capabilities.includes('team.manage'))
const workspace = computed(() => team.value.id)
const inviting = ref(false)

const invitations = useQuery({
  queryKey: computed(() => queryKeys.invitations(workspace.value)),
  queryFn: ({ signal }) => teamsResource.invitations(workspace.value, signal),
  enabled: manage,
})

/** `invalid_arguments` on `user` is the last owner leaving or being demoted (decision 0012). */
const fail = (error: unknown) => {
  if (error instanceof ApiError && error.code === 'stale_revision') void refreshTeam(workspace.value)
  const lastOwner = error instanceof ApiError && error.code === 'invalid_arguments' && !!error.body.errors?.user
  toast.error(lastOwner ? t('members.lastOwner') : message(error))
}

const changeRole = async (member: Member, value: unknown) => {
  const role = assignableRoles(team.value.role).find((entry) => entry === value)
  if (!role || role === member.role) return
  const args = { workspace: workspace.value, user: member.user, role, expected_revision: props.revision }
  try {
    const result = await reauth.run('members.updateRole', args, () => teamsResource.updateRole(args))
    if (!result) return
    toast.success(t('members.roleChanged', { name: member.name, role: t(roleLabels[role]) }))
    await refreshTeam(workspace.value)
  } catch (error) {
    fail(error)
  }
}

const remove = useMutation({
  mutationFn: (member: Member) => teamsResource.removeMember(workspace.value, member.user),
  onSuccess: async (_, member) => {
    if (member.user === account.value?.id) {
      forgetWorkspace(workspace.value)
      toast.success(t('members.left', { team: team.value.name }))
      await router.push({ name: 'home' })
      return
    }
    toast.success(t('members.removed', { name: member.name }))
    await refreshTeam(workspace.value)
  },
  onError: fail,
})

const askRemove = async (member: Member) => {
  const self = member.user === account.value?.id
  const ok = await confirm({
    title: self ? t('members.leave') : t('members.remove'),
    message: self ? t('members.leaveConfirm', { team: team.value.name }) : t('members.removeConfirm', { name: member.name }),
    confirmLabel: self ? t('members.leave') : t('members.remove'),
    variant: 'destructive',
  })
  if (ok) remove.mutate(member)
}

const revoke = useMutation({
  mutationFn: (invitation: Invitation) => teamsResource.revokeInvitation(invitation.id),
  onSuccess: () => {
    toast.success(t('invitations.revoked'))
    return refreshTeam(workspace.value)
  },
  onError: fail,
})

const askRevoke = async (invitation: Invitation) => {
  const ok = await confirm({
    title: t('invitations.revoke'),
    message: t('invitations.revokeConfirm', { label: invitation.label || invitation.prefix }),
    confirmLabel: t('invitations.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(invitation)
}

const stateVariant = (state: Invitation['state']) =>
  state === 'pending' ? 'warning' : state === 'used' ? 'success' : 'secondary'
const roleOf = (role: Role) => t(roleLabels[role])
</script>

<template>
  <div class="grid gap-8">
    <section
      class="grid gap-3"
      aria-labelledby="members-title"
    >
      <h2
        id="members-title"
        class="flex min-h-8 items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
      >
        {{ t('workspace.members') }}
        <span class="text-sm font-normal text-muted tabular-nums">· {{ detail.members.length }}</span>
      </h2>
      <Table :label="t('workspace.members')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('members.person') }}</TableHead>
            <TableHead>{{ t('members.role') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow
            v-for="member in detail.members"
            :key="member.user"
          >
            <TableCell>
              <div class="grid min-w-0 gap-0.5">
                <span class="flex items-center gap-1.5 font-medium text-primary">
                  <span class="truncate">{{ member.name }}</span>
                  <Badge
                    v-if="member.user === account?.id"
                    size="xs"
                  >
                    {{ t('members.you') }}
                  </Badge>
                  <SimpleTooltip
                    v-if="member.two_factor_enrolled"
                    :tooltip="t('members.twoFactorOn')"
                  >
                    <Icon
                      name="lucide:shield-check"
                      class="text-success"
                      :aria-label="t('members.twoFactorOn')"
                    />
                  </SimpleTooltip>
                </span>
                <span class="truncate text-xs text-muted">{{ member.email }}</span>
              </div>
            </TableCell>
            <TableCell>
              <Select
                v-if="manage && member.user !== account?.id"
                :model-value="member.role"
                @update:model-value="(value) => changeRole(member, value)"
              >
                <SelectTrigger
                  class="h-8 w-36"
                  :aria-label="t('members.roleOf', { name: member.name })"
                >
                  <SelectValue>{{ roleOf(member.role) }}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="role in assignableRoles(team.role)"
                    :key="role"
                    :value="role"
                  >
                    {{ roleOf(role) }}
                  </SelectItem>
                </SelectContent>
              </Select>
              <span
                v-else
                class="text-foreground"
              >
                {{ roleOf(member.role) }}
              </span>
            </TableCell>
            <TableCell class="text-right">
              <Button
                v-if="manage || member.user === account?.id"
                variant="ghost"
                size="sm"
                class="text-destructive"
                :aria-label="
                  member.user === account?.id ? t('members.leave') : t('members.removeNamed', { name: member.name })
                "
                @click="askRemove(member)"
              >
                <Icon :name="member.user === account?.id ? 'lucide:log-out' : 'lucide:user-minus'" />
                <span class="max-sm:sr-only">
                  {{ member.user === account?.id ? t('members.leave') : t('members.remove') }}
                </span>
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </section>

    <section
      v-if="manage"
      class="grid gap-3"
      aria-labelledby="invitations-title"
    >
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-1.5">
          <h2
            id="invitations-title"
            class="text-[1.0625rem] font-semibold text-primary"
          >
            {{ t('invitations.title') }}
          </h2>
          <SimpleTooltip
            :tooltip="t('invitations.description')"
            class="text-muted hover:text-primary"
          >
            <Icon
              name="lucide:info"
              aria-hidden="true"
            />
            <span class="sr-only">{{ t('invitations.description') }}</span>
          </SimpleTooltip>
        </div>
        <Button
          variant="fill"
          class="text-sm"
          @click="inviting = true"
        >
          <Icon name="lucide:user-plus" />
          {{ t('invitations.create') }}
        </Button>
      </div>
      <Table :label="t('invitations.title')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('invitations.label') }}</TableHead>
            <TableHead>{{ t('members.role') }}</TableHead>
            <TableHead>{{ t('invitations.state') }}</TableHead>
            <TableHead>{{ t('invitations.expires') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="invitations.isPending.value"
            :colspan="5"
            :rows="2"
          />
          <TableEmptyRow
            v-else-if="(invitations.data.value?.invitations ?? []).length === 0"
            :colspan="5"
            :label="invitations.isError.value ? message(invitations.error.value) : t('invitations.empty')"
          />
          <TableRow
            v-for="invitation in invitations.data.value?.invitations ?? []"
            :key="invitation.id"
          >
            <TableCell>
              <div class="grid min-w-0 gap-0.5">
                <span class="truncate font-medium text-primary">{{ invitation.label || t('invitations.unlabelled') }}</span>
                <span class="font-mono text-xs text-muted">{{ invitation.prefix }}…</span>
              </div>
            </TableCell>
            <TableCell>{{ roleOf(invitation.role) }}</TableCell>
            <TableCell>
              <Badge
                :variant="stateVariant(invitation.state)"
                size="sm"
              >
                {{ t(invitationStateLabels[invitation.state]) }}
              </Badge>
            </TableCell>
            <TableCell
              class="whitespace-nowrap text-muted"
              :title="dateTime(invitation.expires_at)"
            >
              {{ date(invitation.expires_at) }}
            </TableCell>
            <TableCell class="text-right">
              <Button
                v-if="invitation.state === 'pending'"
                variant="ghost"
                size="icon"
                :aria-label="t('invitations.revokeNamed', { label: invitation.label || invitation.prefix })"
                @click="askRevoke(invitation)"
              >
                <Icon name="lucide:trash-2" />
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <InviteDialog
        v-model:open="inviting"
        :team="team"
      />
    </section>
  </div>
</template>
