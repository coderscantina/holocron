<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { sitesResource } from '~/api/resources/sites'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { SitesCreateArgs, WorkspaceId } from '~/generated'
import { SITE_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/** A new site in the workspace. Opens its tracking settings afterwards, where the snippet lives. */
const props = defineProps<{ workspace: WorkspaceId }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const router = useRouter()

const timezones = Intl.supportedValuesOf('timeZone')
const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone

const name = ref('')
const domain = ref('')
const timezone = ref(browserZone)
let intent = crypto.randomUUID()
watch([name, domain, timezone], () => {
  intent = crypto.randomUUID()
})

const create = useMutation({
  mutationFn: (args: SitesCreateArgs) => sitesResource.create(args, intent),
  onSuccess: async ({ site }) => {
    open.value = false
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      queryClient.invalidateQueries({ queryKey: queryKeys.sites }),
      queryClient.invalidateQueries({ queryKey: queryKeys.team(props.workspace) }),
    ])
    await router.push({ name: 'site-settings-tracking', params: { siteId: site.id } })
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  domain.value = ''
  timezone.value = browserZone
  create.reset()
})

const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))
const submit = () =>
  create.mutate({ workspace: props.workspace, name: name.value, domain: domain.value, timezone: timezone.value })
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="max-w-md">
      <DialogHeaderCombined
        :title="t('workspace.addSite')"
        :description="t('workspace.addSiteDescription')"
      />
      <form
        class="grid gap-4"
        novalidate
        @submit.prevent="submit"
      >
        <FormField
          id="new-site-name"
          v-model="name"
          :label="t('settings.name')"
          :description="t('settings.nameHint', { max: SITE_NAME_MAX_CHARS })"
          :error="fieldMessage(error, 'name')"
          required
        />
        <FormField
          id="new-site-domain"
          v-model="domain"
          autocomplete="off"
          :label="t('settings.domain')"
          :description="t('settings.domainHint')"
          :error="fieldMessage(error, 'domain')"
          required
        />
        <div class="grid gap-1.5">
          <Label for="new-site-timezone">{{ t('settings.timezone') }}</Label>
          <Select v-model="timezone">
            <SelectTrigger id="new-site-timezone">
              <SelectValue />
            </SelectTrigger>
            <SelectContent class="max-h-80">
              <SelectItem
                v-for="zone in timezones"
                :key="zone"
                :value="zone"
              >
                {{ zone }}
              </SelectItem>
            </SelectContent>
          </Select>
          <p
            v-if="fieldMessage(error, 'timezone')"
            class="text-xs font-medium text-destructive"
          >
            {{ fieldMessage(error, 'timezone') }}
          </p>
        </div>
        <CommandAlert
          :shown="['name', 'domain', 'timezone']" :error="error" />
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
            :disabled="!name.trim() || !domain.trim()"
          >
            {{ t('workspace.addSite') }}
          </Button>
        </div>
      </form>
    </DialogContent>
  </Dialog>
</template>
