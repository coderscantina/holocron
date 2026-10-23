<script setup lang="ts">
import { toast } from 'vue-sonner'

import { hasUrl, linksResource, sourcePath, type SourceKind } from '~/api/resources/links'
import Icon from '~/components/Icon.vue'
import SourceDialog from '~/components/workspace/SourceDialog.vue'
import { Button } from '~/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import TableLoadingRow from '~/components/ui/TableLoadingRow.vue'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { refreshLinks } from '~/composables/useLinks'
import { useReauth } from '~/composables/useReauth'
import type { Capability, Link, Pixel, WorkspaceId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * The links or the pixels of a workspace: their public address to copy, a report per row, and
 * create, edit and delete for callers whose role allows it. Deleting asks first and needs a
 * fresh password, since it erases the hits.
 */
const props = defineProps<{
  kind: SourceKind
  workspace: WorkspaceId
  items: (Link | Pixel)[]
  capabilities: Capability[]
  canCreate: boolean
  loading: boolean
  error: unknown
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const reauth = useReauth()

const isLink = computed(() => props.kind === 'link')
const canEdit = computed(() => props.capabilities.includes('site.configure'))
const canDelete = computed(() => props.capabilities.includes('site.delete'))
const columns = computed(() => (isLink.value ? 4 : 3))

const dialogOpen = ref(false)
const editing = ref<Link | Pixel | null>(null)
const openDialog = (source: Link | Pixel | null) => {
  editing.value = source
  dialogOpen.value = true
}

const targetOf = (source: Link | Pixel) => (hasUrl(source) ? source.url : '')
const address = (source: Link | Pixel) => `${window.location.origin}${sourcePath(props.kind, source.slug)}`
const copy = async (source: Link | Pixel) => {
  await navigator.clipboard.writeText(address(source))
  toast.success(t('secrets.copied'))
}

const remove = async (source: Link | Pixel) => {
  const ok = await confirm({
    title: isLink.value ? t('links.deleteLink') : t('links.deletePixel'),
    message: t(isLink.value ? 'links.deleteLinkConfirm' : 'links.deletePixelConfirm', {
      name: source.name,
      path: sourcePath(props.kind, source.slug),
    }),
    confirmLabel: t('links.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  try {
    const done =
      props.kind === 'link'
        ? await reauth.run('links.delete', { link: source.id }, () => linksResource.removeLink(source.id))
        : await reauth.run('pixels.delete', { pixel: source.id }, () => linksResource.removePixel(source.id))
    if (!done) return
    toast.success(t('links.deleted', { name: source.name }))
    await refreshLinks(props.workspace)
  } catch (error) {
    toast.error(message(error))
  }
}

const reportRoute = (source: Link | Pixel) => ({
  name: isLink.value ? 'link-report' : 'pixel-report',
  params: { workspaceId: props.workspace, sourceId: source.id },
})
const title = computed(() => (isLink.value ? t('links.links') : t('links.pixels')))
</script>

<template>
  <section
    class="grid gap-3"
    :aria-labelledby="`${kind}-title`"
  >
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="grid gap-0.5">
        <h2
          :id="`${kind}-title`"
          class="flex min-h-8 items-center gap-2.5 text-[1.0625rem] font-semibold text-primary"
        >
          {{ title }}
          <span
            v-if="!loading"
            class="text-sm font-normal text-muted tabular-nums"
          >
            · {{ items.length }}
          </span>
        </h2>
        <p class="text-sm text-muted">{{ isLink ? t('links.linksDescription') : t('links.pixelsDescription') }}</p>
      </div>
      <Button
        v-if="canCreate"
        variant="fill"
        class="text-sm"
        @click="openDialog(null)"
      >
        <Icon name="lucide:plus" />
        {{ isLink ? t('links.addLink') : t('links.addPixel') }}
      </Button>
    </div>
    <Table :label="title">
      <TableHeader>
        <TableRow>
          <TableHead>{{ t('settings.name') }}</TableHead>
          <TableHead>{{ t('links.address') }}</TableHead>
          <TableHead
            v-if="isLink"
            class="max-md:hidden"
          >
            {{ t('links.url') }}
          </TableHead>
          <TableHead><span class="sr-only">{{ t('actions.label') }}</span></TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableLoadingRow
          v-if="loading"
          :colspan="columns"
          :rows="2"
        />
        <TableEmptyRow
          v-else-if="items.length === 0"
          :colspan="columns"
          :label="error ? message(error) : isLink ? t('links.noLinks') : t('links.noPixels')"
        />
        <TableRow
          v-for="source in items"
          :key="source.id"
        >
          <TableCell>
            <RouterLink
              :to="reportRoute(source)"
              class="font-medium text-primary hover:underline focus-visible:underline"
            >
              {{ source.name }}
            </RouterLink>
          </TableCell>
          <TableCell>
            <div class="flex min-w-0 items-center gap-1">
              <span class="truncate font-mono text-xs text-muted">{{ sourcePath(kind, source.slug) }}</span>
              <Button
                variant="ghost"
                size="icon"
                :aria-label="t('links.copyAddress', { name: source.name })"
                @click="copy(source)"
              >
                <Icon name="lucide:copy" />
              </Button>
            </div>
          </TableCell>
          <TableCell
            v-if="isLink"
            class="max-w-80 max-md:hidden"
          >
            <span
              class="block truncate text-muted"
              :title="targetOf(source)"
            >
              {{ targetOf(source) }}
            </span>
          </TableCell>
          <TableCell class="text-right whitespace-nowrap">
            <Button
              v-if="canEdit"
              variant="ghost"
              size="icon"
              :aria-label="t('links.editNamed', { name: source.name })"
              @click="openDialog(source)"
            >
              <Icon name="lucide:pencil" />
            </Button>
            <Button
              v-if="canDelete"
              variant="ghost"
              size="icon"
              class="text-destructive"
              :aria-label="t('links.deleteNamed', { name: source.name })"
              @click="remove(source)"
            >
              <Icon name="lucide:trash-2" />
            </Button>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
    <SourceDialog
      v-model:open="dialogOpen"
      :kind="kind"
      :workspace="workspace"
      :source="editing"
    />
  </section>
</template>
