<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { Textarea } from '~/components/ui/textarea'
import { useConfirm } from '~/composables/useConfirm'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { watchScope } from '~/composables/useExplore'
import type { CollectionKey, ScriptTag, Site } from '~/generated'
import { COLLECTION_KEY_SCHEME } from '~/generated/limits'
import CollectionKeyDialog from '~/pages/settings/CollectionKeyDialog.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * How collection reaches the site: the script tags for its pages, and the collection keys of
 * server integrations, which may name the visitor's address and browser themselves.
 */
const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const { date, dateTime } = useDateFormat()

const canConfigure = computed(() => props.site.capabilities.includes('site.configure'))
const origin = window.location.origin
const creating = ref(false)

const code = useQuery({
  queryKey: computed(() => queryKeys.trackingCode(props.site.id, origin)),
  queryFn: ({ signal }) => sitesResource.trackingCode(props.site.id, origin, signal),
  enabled: canConfigure,
})
const keys = useQuery({
  queryKey: computed(() => queryKeys.collectionKeys(props.site.id)),
  queryFn: ({ signal }) => sitesResource.collectionKeys(props.site.id, signal),
  enabled: canConfigure,
})
watchScope(() => props.site.id, () => code.error.value ?? keys.error.value)

interface Snippet {
  id: string
  label: string
  hint?: string
  tag: ScriptTag
}

const snippets = computed<Snippet[]>(() => {
  const data = code.data.value
  if (!data) return []
  const [tracker, ...others] = data.tracker
  const list: Snippet[] = []
  if (tracker) list.push({ id: 'tracker-code', label: t('settings.tracking.tracker'), hint: t('settings.tracking.trackerHint'), tag: tracker })
  for (const [index, tag] of others.entries()) {
    list.push({ id: `tracker-code-${index}`, label: t('settings.tracking.alternative', { path: tag.path }), tag })
  }
  if (data.recorder) {
    list.push({ id: 'recorder-code', label: t('settings.tracking.recorder'), hint: t('settings.tracking.recorderHint'), tag: data.recorder })
  }
  return list
})

const copy = async (tag: ScriptTag) => {
  await navigator.clipboard.writeText(tag.code)
  toast.success(t('secrets.copied'))
}

const revoke = useMutation({
  mutationFn: (key: CollectionKey) => sitesResource.revokeCollectionKey(props.site.id, key.id),
  onSuccess: () => {
    toast.success(t('collectionKeys.revoked'))
    return queryClient.invalidateQueries({ queryKey: queryKeys.collectionKeys(props.site.id) })
  },
  onError: (error) => toast.error(message(error)),
})

const ask = async (key: CollectionKey) => {
  const ok = await confirm({
    title: t('collectionKeys.revoke'),
    message: t('collectionKeys.revokeConfirm', { name: key.name }),
    confirmLabel: t('collectionKeys.revoke'),
    variant: 'destructive',
  })
  if (ok) revoke.mutate(key)
}

const COLUMNS = 4
</script>

<template>
  <SettingsPage>
    <p
      v-if="!canConfigure"
      class="text-sm text-muted"
    >
      {{ t('settings.tracking.restricted') }}
    </p>
    <template v-else>
      <SettingsSection
        id="tracking-code"
        :title="t('settings.tracking.scripts')"
        :description="t('settings.tracking.description')"
      >
        <Skeleton
          v-if="code.isPending.value"
          class="h-20 w-full"
        />
        <Alert
          v-else-if="code.isError.value"
          color="destructive"
        >
          <p class="text-sm">{{ message(code.error.value) }}</p>
        </Alert>
        <div
          v-for="snippet in snippets"
          v-else
          :key="snippet.id"
          class="grid gap-1.5"
        >
          <Label :for="snippet.id">{{ snippet.label }}</Label>
          <p
            v-if="snippet.hint"
            :id="`${snippet.id}-hint`"
            class="text-xs text-muted"
          >
            {{ snippet.hint }}
          </p>
          <div class="flex flex-col gap-2 sm:flex-row sm:items-start">
            <Textarea
              :id="snippet.id"
              :model-value="snippet.tag.code"
              :aria-describedby="snippet.hint ? `${snippet.id}-hint` : undefined"
              readonly
              auto-size
              class="min-h-0 resize-none font-mono text-xs"
              @focus="($event.target as HTMLTextAreaElement).select()"
            />
            <Button
              type="button"
              variant="fill"
              class="shrink-0"
              @click="copy(snippet.tag)"
            >
              <Icon name="lucide:copy" />
              {{ t('secrets.copy') }}
            </Button>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection
        id="collection-keys"
        :title="t('collectionKeys.title')"
        :description="t('collectionKeys.description', { scheme: COLLECTION_KEY_SCHEME })"
      >
        <Table :label="t('collectionKeys.title')">
          <TableHeader>
            <TableRow>
              <TableHead>{{ t('collectionKeys.name') }}</TableHead>
              <TableHead>{{ t('collectionKeys.created') }}</TableHead>
              <TableHead>{{ t('collectionKeys.state') }}</TableHead>
              <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableLoadingRow
              v-if="keys.isPending.value"
              :colspan="COLUMNS"
              :rows="2"
            />
            <TableEmptyRow
              v-else-if="(keys.data.value?.keys ?? []).length === 0"
              :colspan="COLUMNS"
              :label="keys.isError.value ? message(keys.error.value) : t('collectionKeys.empty')"
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
              <TableCell
                class="whitespace-nowrap text-muted"
                :title="dateTime(key.created_at)"
              >
                {{ date(key.created_at) }}
              </TableCell>
              <TableCell class="whitespace-nowrap">
                <Badge
                  v-if="key.revoked_at"
                  variant="secondary"
                  size="sm"
                >
                  {{ t('collectionKeys.revokedOn', { date: date(key.revoked_at) }) }}
                </Badge>
                <Badge
                  v-else
                  variant="success"
                  size="sm"
                >
                  {{ t('collectionKeys.active') }}
                </Badge>
              </TableCell>
              <TableCell class="text-right">
                <Button
                  v-if="!key.revoked_at"
                  variant="ghost"
                  size="icon"
                  :aria-label="t('collectionKeys.revokeNamed', { name: key.name })"
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
            {{ t('collectionKeys.create') }}
          </Button>
        </template>
        <CollectionKeyDialog
          v-model:open="creating"
          :site="site"
        />
      </SettingsSection>
    </template>
  </SettingsPage>
</template>
