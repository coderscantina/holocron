<script setup lang="ts">
import { toast } from 'vue-sonner'

import { ApiError } from '~/api'
import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { useConfirm } from '~/composables/useConfirm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { forgetSite } from '~/composables/useExplore'
import { useReauth } from '~/composables/useReauth'
import { useSession } from '~/composables/useSession'
import { useSiteContext } from '~/composables/useSiteContext'
import type { Site, SitesTransferArgs, TransferTarget } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Moving a site to another workspace, deleting its data, deleting it. Each one asks for
 * confirmation and a fresh password check (decision 0007); none of them can be undone.
 */
const props = defineProps<{ site: Site }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { confirm } = useConfirm()
const reauth = useReauth()
const router = useRouter()
const { account } = useSession()
const context = useSiteContext()

const can = (capability: Site['capabilities'][number]) => props.site.capabilities.includes(capability)
const busy = ref(false)

const PERSONAL = 'personal'
const transferring = ref(false)
const target = ref('')
/** Teams other than the site's own, and the caller's personal workspace unless the site is already there. */
const targets = computed(() => {
  const own = context.workspaces.value.find((workspace) => workspace.id === props.site.workspace)
  const teams = context.workspaces.value
    .filter((workspace) => workspace.kind === 'team' && workspace.id !== props.site.workspace)
    .map((workspace) => ({ value: workspace.id, label: workspace.name }))
  return own?.kind === 'personal' ? teams : [{ value: PERSONAL, label: t('siteManage.personal') }, ...teams]
})
const targetLabel = computed(() => targets.value.find((entry) => entry.value === target.value)?.label ?? '')

const leave = async (done: string) => {
  forgetSite(props.site.id)
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
    queryClient.invalidateQueries({ queryKey: queryKeys.sites }),
  ])
  toast.success(done)
}

const transfer = async () => {
  const to: TransferTarget | null =
    target.value === PERSONAL
      ? account.value
        ? { direction: 'to_user', user: account.value.id }
        : null
      : target.value
        ? { direction: 'to_team', team: target.value }
        : null
  if (!to) return
  const args: SitesTransferArgs = { site: props.site.id, to }
  busy.value = true
  try {
    const result = await reauth.run('sites.transfer', args, () => sitesResource.transfer(args))
    if (!result) return
    transferring.value = false
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.site(props.site.id) }),
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      queryClient.invalidateQueries({ queryKey: queryKeys.shares(props.site.id) }),
    ])
    toast.success(t('siteManage.transferred', { site: props.site.name, target: targetLabel.value }))
  } catch (error) {
    const refused = error instanceof ApiError && error.code === 'invalid_arguments' && !!error.body.errors?.to
    toast.error(refused ? t('siteManage.transferRefused') : message(error))
  } finally {
    busy.value = false
  }
}

const reset = async () => {
  const ok = await confirm({
    title: t('siteManage.reset'),
    message: t('siteManage.resetConfirm', { site: props.site.name }),
    confirmLabel: t('siteManage.reset'),
    variant: 'destructive',
  })
  if (!ok) return
  const args = { site: props.site.id }
  busy.value = true
  try {
    const result = await reauth.run('sites.reset', args, () => sitesResource.reset(props.site.id))
    if (!result) return
    // The site stays; every analytical result of it is gone.
    queryClient.removeQueries({ predicate: (query) => query.queryKey[1] === props.site.id && query.queryKey[0] !== 'sites.get' })
    toast.success(t('siteManage.resetDone', { site: props.site.name }))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}

const remove = async () => {
  const ok = await confirm({
    title: t('siteManage.delete'),
    message: t('siteManage.deleteConfirm', { site: props.site.name }),
    confirmLabel: t('siteManage.delete'),
    variant: 'destructive',
  })
  if (!ok) return
  const args = { site: props.site.id }
  busy.value = true
  try {
    const result = await reauth.run('sites.delete', args, () => sitesResource.remove(props.site.id))
    if (!result) return
    await router.push({ name: 'workspace', params: { workspaceId: props.site.workspace } })
    await leave(t('siteManage.deleted', { site: props.site.name }))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SettingsSection
    v-if="can('site.transfer') || can('site.delete')"
    id="site-management"
    :title="t('siteManage.title')"
    :description="t('siteManage.description')"
    destructive
  >
    <ul class="-mt-3 divide-y divide-divider">
      <li
        v-if="can('site.transfer')"
        class="flex flex-wrap items-center justify-between gap-3 py-3"
      >
        <div class="min-w-0">
          <p class="text-sm font-medium text-primary">{{ t('siteManage.transfer') }}</p>
          <p class="text-xs text-muted">{{ t('siteManage.transferHint') }}</p>
        </div>
        <Button
          variant="fill"
          :disabled="targets.length === 0"
          @click="transferring = true"
        >
          {{ t('siteManage.transfer') }}
        </Button>
      </li>
      <template v-if="can('site.delete')">
        <li class="flex flex-wrap items-center justify-between gap-3 py-3">
          <div class="min-w-0">
            <p class="text-sm font-medium text-primary">{{ t('siteManage.reset') }}</p>
            <p class="text-xs text-muted">{{ t('siteManage.resetHint') }}</p>
          </div>
          <Button
            variant="destructive"
            :loading="busy"
            @click="reset"
          >
            {{ t('siteManage.reset') }}
          </Button>
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3">
          <div class="min-w-0">
            <p class="text-sm font-medium text-primary">{{ t('siteManage.delete') }}</p>
            <p class="text-xs text-muted">{{ t('siteManage.deleteHint') }}</p>
          </div>
          <Button
            variant="destructive"
            :loading="busy"
            @click="remove"
          >
            {{ t('siteManage.delete') }}
          </Button>
        </li>
      </template>
    </ul>

    <Dialog v-model:open="transferring">
      <DialogContent class="max-w-md">
        <DialogHeaderCombined
          :title="t('siteManage.transfer')"
          :description="t('siteManage.transferDescription', { site: site.name })"
        />
        <form
          class="grid gap-4"
          @submit.prevent="transfer"
        >
          <div class="grid gap-1.5">
            <Label for="transfer-target">{{ t('siteManage.target') }}</Label>
            <Select v-model="target">
              <SelectTrigger id="transfer-target">
                <SelectValue :placeholder="t('siteManage.chooseTarget')">{{ targetLabel }}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="entry in targets"
                  :key="entry.value"
                  :value="entry.value"
                >
                  {{ entry.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              @click="transferring = false"
            >
              {{ t('actions.cancel') }}
            </Button>
            <Button
              type="submit"
              variant="primary"
              :loading="busy"
              :disabled="!target"
            >
              {{ t('siteManage.transfer') }}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  </SettingsSection>
</template>
