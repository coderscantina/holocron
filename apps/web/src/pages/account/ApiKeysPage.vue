<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/AccessTokensSection.vue, as a
// table with operation and site grants.
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import ApiKeyDialog from '~/components/account/ApiKeyDialog.vue'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { usePageMeta } from '~/composables/usePageMeta'
import { useSiteList } from '~/composables/useSiteContext'
import type { ApiKey } from '~/generated'
import { API_KEY_SCHEME } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Scoped API keys. A key acts as its owner, limited to its operations and sites; the owner's
 * current role still applies on top, so a key can only ever do less than its owner.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()
usePageMeta(() => t('apiKeys.title'))

const keys = useQuery({ queryKey: queryKeys.apiKeys, queryFn: ({ signal }) => accountResource.apiKeys(signal) })
const sites = useSiteList()
const creating = ref(false)

const revoke = useMutation({
  mutationFn: (key: ApiKey) => accountResource.revokeApiKey(key.id),
  onSuccess: () => {
    toast.success(t('apiKeys.revoked'))
    return queryClient.invalidateQueries({ queryKey: queryKeys.apiKeys })
  },
  onError: (error) => toast.error(message(error)),
})

const ask = async (key: ApiKey) => {
  const ok = await confirm({
    title: t('apiKeys.revoke'),
    message: t('apiKeys.revokeConfirm', { name: key.name }),
    confirmLabel: t('apiKeys.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(key)
}

const siteNames = (key: ApiKey): string => {
  if (key.sites.kind === 'all') return t('apiKeys.allSites')
  const known = new Map((sites.data.value?.sites ?? []).map((site) => [site.id, site.name]))
  return key.sites.sites.map((id) => known.get(id) ?? id).join(', ')
}

const COLUMNS = 5
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="keys"
      :title="t('apiKeys.list')"
      :description="t('apiKeys.description', { scheme: API_KEY_SCHEME })"
    >
      <Table :label="t('apiKeys.list')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('apiKeys.name') }}</TableHead>
            <TableHead>{{ t('apiKeys.operations') }}</TableHead>
            <TableHead>{{ t('apiKeys.sites') }}</TableHead>
            <TableHead>{{ t('apiKeys.lastUsed') }}</TableHead>
            <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableLoadingRow
            v-if="keys.isPending.value"
            :colspan="COLUMNS"
            :rows="3"
          />
          <TableEmptyRow
            v-else-if="(keys.data.value?.keys ?? []).length === 0"
            :colspan="COLUMNS"
            :label="keys.isError.value ? message(keys.error.value) : t('apiKeys.empty')"
          />
          <TableRow
            v-for="key in keys.data.value?.keys ?? []"
            :key="key.id"
          >
            <TableCell>
              <div class="grid min-w-0 gap-0.5">
                <span
                  class="truncate font-medium"
                  :class="key.revoked_at ? 'text-muted' : 'text-primary'"
                >{{ key.name }}</span>
                <span class="font-mono text-xs text-muted">{{ key.prefix }}…</span>
              </div>
            </TableCell>
            <TableCell>
              <div class="flex max-w-xs flex-wrap gap-1">
                <Badge
                  v-for="operation in key.operations"
                  :key="operation"
                  size="sm"
                  class="font-mono"
                >
                  {{ operation }}
                </Badge>
              </div>
            </TableCell>
            <TableCell class="min-w-32 text-muted">{{ siteNames(key) }}</TableCell>
            <TableCell class="whitespace-nowrap text-muted">
              <Badge
                v-if="key.revoked_at"
                variant="destructive"
                size="sm"
              >
                {{ t('apiKeys.revokedOn', { date: date(key.revoked_at) }) }}
              </Badge>
              <span
                v-else-if="key.last_used_at"
                :title="dateTime(key.last_used_at)"
              >
                {{ date(key.last_used_at) }}
              </span>
              <span v-else>{{ t('apiKeys.neverUsed') }}</span>
            </TableCell>
            <TableCell class="text-right">
              <Button
                v-if="!key.revoked_at"
                variant="ghost"
                size="icon"
                :aria-label="t('apiKeys.revokeNamed', { name: key.name })"
                @click="ask(key)"
              >
                <Icon name="lucide:trash-2" />
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
      <template #footer>
        <Button
          variant="fill"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('apiKeys.create') }}
        </Button>
      </template>
    </SettingsSection>
    <ApiKeyDialog v-model:open="creating" />
  </SettingsPage>
</template>
