<script setup lang="ts">
import { useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import ResetCredentialsDialog from '~/components/admin/ResetCredentialsDialog.vue'
import UserDialog from '~/components/admin/UserDialog.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useSession } from '~/composables/useSession'
import type { OwnedResource, UserSummary } from '~/generated'
import { installRoleLabels } from '~/lib/roles'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Every account of the installation. Deleting refuses while the person still owns a workspace
 * or site, and for the last install admin; the refusal says what is in the way.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date } = useDateFormat()
const reauth = useReauth()
const { account } = useSession()

const users = useQuery({ queryKey: queryKeys.users, queryFn: ({ signal }) => adminResource.users(signal) })

const editing = ref<UserSummary | null>(null)
const editOpen = ref(false)
const resetting = ref<UserSummary | null>(null)
const resetOpen = ref(false)
const blocked = ref<{ user: UserSummary; owned: OwnedResource[] } | null>(null)

const openEditor = (user: UserSummary | null) => {
  editing.value = user
  editOpen.value = true
}
const openReset = (user: UserSummary) => {
  resetting.value = user
  resetOpen.value = true
}

const remove = async (user: UserSummary) => {
  const ok = await confirm({
    title: t('admin.users.delete'),
    message: t('admin.users.deleteConfirm', { name: user.name, email: user.email }),
    confirmLabel: t('admin.users.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  const args = { user: user.id }
  try {
    const result = await reauth.run('users.delete', args, () => adminResource.deleteUser(user.id))
    if (!result) return
    toast.success(t('admin.users.deleted', { name: user.name }))
    await queryClient.invalidateQueries({ queryKey: queryKeys.users })
  } catch (error) {
    if (error instanceof ApiError && error.code === 'user_owns_resources') {
      blocked.value = { user, owned: error.body.owned ?? [] }
      return
    }
    toast.error(error instanceof ApiError && error.code === 'last_install_admin' ? t('admin.users.lastAdmin') : message(error))
  }
}

const COLUMNS = 5
</script>

<template>
  <div class="grid gap-4">
    <div class="flex justify-end">
      <Button
        variant="fill"
        @click="openEditor(null)"
      >
        <Icon name="lucide:user-plus" />
        {{ t('admin.users.create') }}
      </Button>
    </div>
    <Table :label="t('admin.users.title')">
      <TableHeader>
        <TableRow>
          <TableHead>{{ t('members.person') }}</TableHead>
          <TableHead>{{ t('account.installRole') }}</TableHead>
          <TableHead>{{ t('twoFactor.title') }}</TableHead>
          <TableHead>{{ t('admin.users.created') }}</TableHead>
          <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="users.isPending.value"
          :colspan="COLUMNS"
          :rows="4"
        />
        <TableEmptyRow
          v-else-if="(users.data.value?.users ?? []).length === 0"
          :colspan="COLUMNS"
          :label="users.isError.value ? message(users.error.value) : t('admin.users.empty')"
        />
        <TableRow
          v-for="user in users.data.value?.users ?? []"
          :key="user.id"
        >
          <TableCell>
            <div class="grid min-w-0 gap-0.5">
              <span class="truncate font-medium text-primary">{{ user.name }}</span>
              <span class="truncate text-xs text-muted">{{ user.email }}</span>
            </div>
          </TableCell>
          <TableCell>
            <Badge
              :variant="user.install_role === 'install_admin' ? 'info' : 'surface'"
              size="sm"
            >
              {{ t(installRoleLabels[user.install_role]) }}
            </Badge>
          </TableCell>
          <TableCell>
            <div class="flex flex-wrap gap-1">
              <Badge
                :variant="user.two_factor_enrolled ? 'success' : 'secondary'"
                size="sm"
              >
                {{ user.two_factor_enrolled ? t('twoFactor.on') : t('twoFactor.off') }}
              </Badge>
              <Badge
                v-if="user.two_factor_required"
                variant="warning"
                size="sm"
              >
                {{ t('twoFactor.required') }}
              </Badge>
            </div>
          </TableCell>
          <TableCell class="whitespace-nowrap text-muted">{{ date(user.created_at) }}</TableCell>
          <TableCell class="text-right">
            <DropdownMenu>
              <DropdownMenuTrigger as-child>
                <Button
                  variant="ghost"
                  size="icon"
                  :aria-label="t('admin.users.actionsFor', { name: user.name })"
                >
                  <Icon name="lucide:ellipsis" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem @select="openEditor(user)">
                  <Icon name="lucide:pencil" />
                  {{ t('admin.users.edit') }}
                </DropdownMenuItem>
                <DropdownMenuItem @select="openReset(user)">
                  <Icon name="lucide:key-round" />
                  {{ t('admin.users.reset') }}
                </DropdownMenuItem>
                <template v-if="user.id !== account?.id">
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    class="text-destructive"
                    @select="remove(user)"
                  >
                    <Icon name="lucide:trash-2" />
                    {{ t('admin.users.delete') }}
                  </DropdownMenuItem>
                </template>
              </DropdownMenuContent>
            </DropdownMenu>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>

    <UserDialog
      v-model:open="editOpen"
      :user="editing"
    />
    <ResetCredentialsDialog
      v-model:open="resetOpen"
      :user="resetting"
    />
    <Dialog
      :open="blocked !== null"
      @update:open="(value) => !value && (blocked = null)"
    >
      <DialogContent class="max-w-md">
        <DialogHeaderCombined
          :title="t('admin.users.blockedTitle')"
          :description="t('admin.users.blockedDescription', { name: blocked?.user.name ?? '' })"
        />
        <Alert color="warning">
          <ul class="grid gap-1 text-sm">
            <li
              v-for="resource in blocked?.owned ?? []"
              :key="`${resource.kind}:${resource.id}`"
              class="flex items-center gap-2"
            >
              <Icon
                :name="resource.kind === 'site' ? 'lucide:globe' : 'lucide:building-2'"
                aria-hidden="true"
              />
              {{ resource.kind === 'site' ? t('admin.users.ownedSite', { name: resource.name }) : t('admin.users.ownedWorkspace', { name: resource.name }) }}
            </li>
          </ul>
        </Alert>
        <div class="flex justify-end">
          <Button
            variant="primary"
            @click="blocked = null"
          >
            {{ t('actions.close') }}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>
