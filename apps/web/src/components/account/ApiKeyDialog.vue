<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/account/AccessTokenDialog.vue.
// Abilities become operation and site grants from the generated `keyGrantable*` lists; keys
// do not expire, they are revoked.
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { accountResource } from '~/api/resources/account'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Checkbox } from '~/components/ui/checkbox'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { RadioGroup, RadioGroupItem } from '~/components/ui/radio-group'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useSiteList } from '~/composables/useSiteContext'
import type { ApiKeyCreated, ApiKeysCreateArgs, KeySites, SiteId } from '~/generated'
import { keyGrantableCommands } from '~/generated/commands'
import { LABEL_MAX_CHARS } from '~/generated/limits'
import { keyGrantableQueries, readAnalyticsPreset } from '~/generated/queries'
import { isReadAnalyticsPreset } from '~/lib/api-key-preset'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/**
 * Creates a key in two steps: choose what it may do, then copy it. The second step is the only
 * time the secret exists outside its hash, so the dialog says so and does not close on an
 * outside click.
 */
const open = defineModel<boolean>('open', { default: false })

type Operation = (typeof keyGrantableQueries)[number] | (typeof keyGrantableCommands)[number]

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const reauth = useReauth()
const sites = useSiteList()

const name = ref('')
const reads = ref<Operation[]>([])
const writes = ref<Operation[]>([])
const scope = ref<KeySites['kind']>('all')
const chosenSites = ref<SiteId[]>([])
const created = ref<ApiKeyCreated | null>(null)

const create = useMutation({
  mutationFn: async (args: ApiKeysCreateArgs) =>
    reauth.run('apiKeys.create', args, () => accountResource.createApiKey(args)),
  onSuccess: (result) => {
    if (!result) return
    created.value = result
    void queryClient.invalidateQueries({ queryKey: queryKeys.apiKeys })
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  reads.value = []
  writes.value = []
  scope.value = 'all'
  chosenSites.value = []
  created.value = null
  create.reset()
})

const operations = computed(() => [...reads.value, ...writes.value])
const presetOn = computed(() => isReadAnalyticsPreset(operations.value))
const togglePreset = (on: boolean | 'indeterminate') => {
  reads.value = on === true ? [...readAnalyticsPreset] : []
  writes.value = []
}
const ready = computed(
  () =>
    name.value.trim() !== '' &&
    operations.value.length > 0 &&
    (scope.value === 'all' || chosenSites.value.length > 0)
)

const submit = () => {
  const keySites: KeySites = scope.value === 'all' ? { kind: 'all' } : { kind: 'only', sites: chosenSites.value }
  create.mutate({ name: name.value, operations: operations.value, sites: keySites })
}

const readOptions = keyGrantableQueries.map((value) => ({ value, label: value, mono: true }))
const writeOptions = keyGrantableCommands.map((value) => ({ value, label: value, mono: true }))
const siteOptions = computed(() =>
  (sites.data.value?.sites ?? []).map((site) => ({ value: site.id, label: `${site.name} · ${site.workspace_name}` }))
)
const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent
      class="max-w-2xl"
      @interact-outside="(event) => created && event.preventDefault()"
    >
      <template v-if="created">
        <DialogHeaderCombined
          :title="t('apiKeys.createdTitle')"
          :description="t('apiKeys.createdDescription')"
        />
        <SecretReveal
          id="api-key-secret"
          :label="t('apiKeys.secret')"
          :secret="created.secret"
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
          :title="t('apiKeys.create')"
          :description="t('apiKeys.createDescription')"
        />
        <form
          class="grid gap-5"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="api-key-name"
            v-model="name"
            autocomplete="off"
            :label="t('apiKeys.name')"
            :description="t('apiKeys.nameHint', { max: LABEL_MAX_CHARS })"
            :error="fieldMessage(error, 'name')"
            required
          />
          <div class="grid gap-1 rounded-lg border border-border p-3">
            <label class="flex w-fit items-center gap-2 text-sm font-medium text-primary">
              <Checkbox
                :model-value="presetOn"
                @update:model-value="togglePreset"
              />
              {{ t('apiKeys.presetReadAnalytics') }}
              <span class="text-xs font-normal text-muted">{{ t('apiKeys.recommended') }}</span>
            </label>
            <p class="text-xs text-muted">{{ t('apiKeys.presetReadAnalyticsHint') }}</p>
          </div>
          <CheckboxGroup
            v-model="reads"
            :legend="t('apiKeys.reads')"
            :options="readOptions"
          />
          <CheckboxGroup
            v-model="writes"
            :legend="t('apiKeys.writes')"
            :options="writeOptions"
            :error="fieldMessage(error, 'operations')"
          />
          <fieldset class="grid gap-2">
            <legend class="mb-1 text-sm font-medium text-primary">{{ t('apiKeys.sites') }}</legend>
            <RadioGroup
              v-model="scope"
              class="gap-2"
            >
              <div class="flex items-center gap-2">
                <RadioGroupItem
                  id="api-key-sites-all"
                  value="all"
                />
                <Label for="api-key-sites-all">{{ t('apiKeys.allSites') }}</Label>
              </div>
              <div class="flex items-center gap-2">
                <RadioGroupItem
                  id="api-key-sites-only"
                  value="only"
                />
                <Label for="api-key-sites-only">{{ t('apiKeys.onlySites') }}</Label>
              </div>
            </RadioGroup>
            <p class="text-xs text-muted">{{ t('apiKeys.sitesHint') }}</p>
          </fieldset>
          <CheckboxGroup
            v-if="scope === 'only'"
            v-model="chosenSites"
            :legend="t('apiKeys.chooseSites')"
            :options="siteOptions"
            :error="fieldMessage(error, 'sites')"
          />
          <CommandAlert
          :shown="['name', 'operations', 'sites']" :error="error" />
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
              :disabled="!ready"
            >
              {{ t('apiKeys.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
