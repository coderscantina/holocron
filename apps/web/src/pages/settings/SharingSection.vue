<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { sharingResource } from '~/api/resources/sharing'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import Icon from '~/components/Icon.vue'
import ShareCreateDialog from '~/components/sharing/ShareCreateDialog.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { watchScope } from '~/composables/useExplore'
import type { Share, Site } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'
import { shareStatus, type ShareStatus } from '~/services/sharing'

/**
 * Restricted shares of the site. A share keeps a copy of its view and limits; revoking it ends
 * every viewer's access at their next read, export or live frame.
 */
const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()

const canShare = computed(() => props.site.capabilities.includes('site.share'))
const creating = ref(false)

const shares = useQuery({
  queryKey: computed(() => queryKeys.shares(props.site.id)),
  queryFn: ({ signal }) => sharingResource.list(props.site.id, signal),
  enabled: canShare,
})
watchScope(() => props.site.id, () => shares.error.value)

const revoke = useMutation({
  mutationFn: (share: Share) => sharingResource.revoke(share.id),
  onSuccess: () => {
    toast.success(t('sharing.revoked'))
    return queryClient.invalidateQueries({ queryKey: queryKeys.shares(props.site.id) })
  },
  onError: (error) => toast.error(message(error)),
})

const ask = async (share: Share) => {
  const ok = await confirm({
    title: t('sharing.revoke'),
    message: t('sharing.revokeConfirm', { name: share.name }),
    confirmLabel: t('sharing.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(share)
}

const statusLabels: Record<ShareStatus, MessageKey> = {
  active: 'sharing.states.active',
  expired: 'sharing.states.expired',
  revoked: 'sharing.states.revoked',
}
const statusVariant = (status: ShareStatus) => (status === 'active' ? 'success' : 'secondary')
const COLUMNS = 5
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="shares"
      :title="t('sharing.title')"
      :description="canShare ? t('sharing.description') : t('sharing.readOnly')"
    >
      <Table
        v-if="canShare"
        :label="t('sharing.title')"
      >
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('sharing.name') }}</TableHead>
            <TableHead>{{ t('sharing.grants') }}</TableHead>
            <TableHead>{{ t('sharing.state') }}</TableHead>
            <TableHead>{{ t('sharing.expires') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="shares.isPending.value"
            :colspan="COLUMNS"
            :rows="3"
          />
          <TableEmptyRow
            v-else-if="(shares.data.value?.shares ?? []).length === 0"
            :colspan="COLUMNS"
            :label="shares.isError.value ? message(shares.error.value) : t('sharing.empty')"
          />
          <TableRow
            v-for="share in shares.data.value?.shares ?? []"
            :key="share.id"
          >
            <TableCell>
              <div class="grid min-w-0 gap-0.5">
                <span class="truncate font-medium text-primary">{{ share.name }}</span>
                <span class="font-mono text-xs text-muted">{{ share.prefix }}…</span>
              </div>
            </TableCell>
            <TableCell>
              <div class="flex flex-wrap gap-1 whitespace-nowrap">
                <Badge
                  v-if="share.target.kind === 'exploration'"
                  size="sm"
                >
                  {{ t('sharing.measureCount', { count: share.target.limits.measures.length }) }}
                </Badge>
                <Badge
                  v-if="share.target.kind === 'exploration'"
                  size="sm"
                >
                  {{ t('sharing.rangeDays', { days: share.target.limits.range_max_days }) }}
                </Badge>
                <Badge
                  v-if="share.export"
                  size="sm"
                  variant="info"
                >
                  {{ t('sharing.exportBadge') }}
                </Badge>
                <Badge
                  v-if="share.recordings"
                  size="sm"
                  variant="info"
                >
                  {{ t('sharing.recordingsBadge') }}
                </Badge>
              </div>
            </TableCell>
            <TableCell>
              <Badge
                :variant="statusVariant(shareStatus(share))"
                size="sm"
              >
                {{ t(statusLabels[shareStatus(share)]) }}
              </Badge>
            </TableCell>
            <TableCell
              class="whitespace-nowrap text-muted"
              :title="dateTime(share.expires_at)"
            >
              {{ date(share.expires_at) }}
            </TableCell>
            <TableCell class="text-right">
              <Button
                v-if="shareStatus(share) === 'active'"
                variant="ghost"
                size="icon"
                :aria-label="t('sharing.revokeNamed', { name: share.name })"
                @click="ask(share)"
              >
                <Icon name="lucide:trash-2" />
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <template
        v-if="canShare"
        #footer
      >
        <Button
          variant="fill"
          @click="creating = true"
        >
          <Icon name="lucide:share-2" />
          {{ t('sharing.create') }}
        </Button>
      </template>
      <ShareCreateDialog
        v-if="canShare"
        v-model:open="creating"
        :site="site"
      />
    </SettingsSection>
  </SettingsPage>
</template>
