<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { explorationsResource } from '~/api/resources/explorations'
import { sitesResource } from '~/api/resources/sites'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import { Textarea } from '~/components/ui/textarea'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useLiveSnapshot } from '~/composables/useLive'
import type { SiteId, SitesCreateArgs } from '~/generated'
import { SITE_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'

/** The first site in the Default team, its tracking code, and a live check for its first hit. */
const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const timezones = Intl.supportedValuesOf('timeZone')
const name = ref('')
const domain = ref('')
const timezone = ref(Intl.DateTimeFormat().resolvedOptions().timeZone)
const site = ref<SiteId | null>(null)

const workspaces = useQuery({
  queryKey: queryKeys.workspaces,
  queryFn: ({ signal }) => sitesResource.workspaces(signal),
})
/** Init's Default team: the admin's only team workspace. */
const team = computed(() => workspaces.data.value?.workspaces.find((workspace) => workspace.kind === 'team'))

const intent = crypto.randomUUID()
const create = useMutation({
  mutationFn: (args: SitesCreateArgs) => sitesResource.create(args, intent),
  onSuccess: async (result) => {
    site.value = result.site.id
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      queryClient.invalidateQueries({ queryKey: queryKeys.sites }),
    ])
  },
})
const error = computed(() => (create.error.value instanceof Error ? create.error.value : null))
const submit = () => {
  if (!team.value) return
  create.mutate({ workspace: team.value.id, name: name.value, domain: domain.value, timezone: timezone.value })
}

const origin = window.location.origin
const code = useQuery({
  queryKey: computed(() => queryKeys.trackingCode(site.value ?? ('' as SiteId), origin)),
  queryFn: ({ signal }) => sitesResource.trackingCode(site.value as SiteId, origin, signal),
  enabled: computed(() => site.value !== null),
})
const tracker = computed(() => code.data.value?.tracker[0]?.code ?? '')

const live = useLiveSnapshot(site, (id, signal) => explorationsResource.realtime(id as SiteId, signal))
const firstHit = computed(() => live.data.value?.recent[0] ?? null)

const copy = async () => {
  await navigator.clipboard.writeText(tracker.value)
  toast.success(t('secrets.copied'))
}
</script>

<template>
  <form
    v-if="!site"
    class="grid gap-7"
    novalidate
    @submit.prevent="submit"
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.site.title') }}
      <span class="block font-medium text-muted">{{ t('setup.site.subtitle') }}</span>
    </h1>
    <div class="grid gap-3.5">
      <FormField
        id="setup-site-name"
        v-model="name"
        :label="t('settings.name')"
        :description="t('settings.nameHint', { max: SITE_NAME_MAX_CHARS })"
        :error="fieldMessage(error, 'name')"
        required
      />
      <FormField
        id="setup-site-domain"
        v-model="domain"
        autocomplete="off"
        :label="t('settings.domain')"
        :description="t('settings.domainHint')"
        :error="fieldMessage(error, 'domain')"
        required
      />
      <div class="grid gap-1.5">
        <Label for="setup-site-timezone">{{ t('settings.timezone') }}</Label>
        <Select v-model="timezone">
          <SelectTrigger id="setup-site-timezone">
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
      </div>
      <CommandAlert
        :shown="['name', 'domain', 'timezone']"
        :error="error"
      />
      <Button
        type="submit"
        variant="accent"
        size="lg"
        :loading="create.isPending.value"
        :disabled="!team || !name.trim() || !domain.trim()"
      >
        {{ t('workspace.addSite') }}
      </Button>
    </div>
  </form>
  <div
    v-else
    class="grid gap-7"
    data-wide
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.site.trackTitle', { name }) }}
      <span class="block font-medium text-muted">{{ t('settings.tracking.description') }}</span>
    </h1>
    <div class="grid gap-1.5">
      <Label for="setup-tracker">{{ t('settings.tracking.tracker') }}</Label>
      <div class="flex flex-col gap-2 sm:flex-row sm:items-start">
        <Textarea
          id="setup-tracker"
          :model-value="tracker"
          readonly
          auto-size
          class="min-h-0 resize-none font-mono text-xs"
          @focus="($event.target as HTMLTextAreaElement).select()"
        />
        <Button
          type="button"
          variant="fill"
          class="shrink-0"
          :disabled="!tracker"
          @click="copy"
        >
          <Icon name="lucide:copy" />
          {{ t('secrets.copy') }}
        </Button>
      </div>
    </div>
    <div
      class="flex items-center gap-3 rounded-[var(--radius-control)] bg-fill p-4 text-sm"
      role="status"
    >
      <span
        class="size-2 shrink-0 rounded-full"
        :class="firstHit ? 'bg-success' : 'animate-pulse bg-accent'"
        aria-hidden="true"
      />
      <span
        v-if="firstHit"
        class="text-primary"
      >
        {{ t('setup.site.firstHit', { path: firstHit.path ?? firstHit.event_name ?? '/' }) }}
      </span>
      <span
        v-else
        class="text-muted"
      >
        {{ t('setup.site.waiting', { domain }) }}
      </span>
    </div>
    <Button
      variant="accent"
      size="lg"
      as-child
    >
      <RouterLink :to="{ name: 'overview', params: { siteId: site } }">
        {{ t('setup.site.open') }}
      </RouterLink>
    </Button>
  </div>
</template>
