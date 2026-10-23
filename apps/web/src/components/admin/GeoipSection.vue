<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'
import { toast } from 'vue-sonner'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import SettingsSection from '~/components/account/SettingsSection.vue'
import DbipAttribution from '~/components/DbipAttribution.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { GeoipFailure } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'
import { runtimeConfig } from '~/services/runtime-config'

/**
 * Where visitor locations come from when no CDN header has one (decision 0018): the operator's
 * file from `HOLOCRON_GEOIP_DB`, or DB-IP Lite City downloaded here. The server runs the
 * download; the settings are polled until it ends.
 */
const POLL_MS = 2000

const failures = {
  unreachable: 'admin.geoip.failed.unreachable',
  not_published: 'admin.geoip.failed.notPublished',
  invalid: 'admin.geoip.failed.invalid',
  disk_full: 'admin.geoip.failed.diskFull',
} as const satisfies Record<GeoipFailure, MessageKey>

const { t, locale } = useI18n()
const { message } = useErrorMessage()
const { date } = useDateFormat()

const settings = useQuery({
  queryKey: queryKeys.geoipSettings,
  queryFn: ({ signal }) => adminResource.geoipSettings(signal),
  refetchInterval: (query) => (query.state.data?.download.state === 'running' ? POLL_MS : false),
})
const download = useMutation({
  mutationFn: adminResource.downloadGeoip,
  onSuccess: (answer) => queryClient.setQueryData(queryKeys.geoipSettings, answer),
  onError: (error) => toast.error(message(error)),
})

const data = computed(() => settings.data.value)
const running = computed(() => data.value?.download.state === 'running')
const failure = computed(() => {
  const state = data.value?.download
  return state?.state === 'failed' ? failures[state.reason] : undefined
})
const newer = computed(() => {
  const latest = data.value?.latest_month
  const installed = data.value?.database?.month
  return latest && installed && latest > installed ? latest : undefined
})

const monthName = (month: string) =>
  new Intl.DateTimeFormat(locale.value, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(
    Date.UTC(Number(month.slice(0, 4)), Number(month.slice(5, 7)) - 1)
  )

watch(data, (value) => {
  if (value) runtimeConfig.dbip_attribution = value.database?.dbip ?? false
})
watch(running, (now, before) => {
  if (before && !now && data.value?.download.state === 'idle') toast.success(t('admin.geoip.installed'))
})
</script>

<template>
  <SettingsSection
    id="visitor-location"
    :title="t('admin.geoip.title')"
    :description="t('admin.geoip.description')"
  >
    <p
      v-if="settings.isError.value"
      class="text-sm text-destructive"
    >
      {{ message(settings.error.value) }}
    </p>
    <template v-else-if="data">
      <template v-if="data.external">
        <p class="text-sm text-foreground">{{ t('admin.geoip.external') }}</p>
        <Alert
          v-if="!data.database"
          color="warning"
          icon="lucide:triangle-alert"
        >
          {{ t('admin.geoip.externalBroken') }}
        </Alert>
      </template>
      <p
        v-else-if="data.database?.month"
        class="text-sm text-foreground"
      >
        {{ t('admin.geoip.current', { month: monthName(data.database.month) }) }}
        <template v-if="newer">{{ t('admin.geoip.newer', { month: monthName(newer) }) }}</template>
      </p>
      <p
        v-else
        class="text-sm text-foreground"
      >
        {{ t('admin.geoip.none') }}
      </p>
      <p
        v-if="data.database"
        class="text-xs text-muted"
      >
        {{ t('admin.geoip.database', { type: data.database.database_type, date: date(data.database.built_at) }) }}
      </p>
      <Alert
        v-if="failure"
        color="destructive"
        icon="lucide:triangle-alert"
      >
        {{ t(failure) }}
      </Alert>
      <p
        v-if="data.database?.dbip"
        class="text-xs text-muted"
      >
        {{ t('admin.geoip.licence') }}
        <DbipAttribution class="text-primary" />
      </p>
    </template>
    <template
      v-if="data && !data.external"
      #footer
    >
      <Button
        variant="accent"
        :loading="running || download.isPending.value"
        @click="download.mutate()"
      >
        {{ data.database ? t('admin.geoip.update') : t('admin.geoip.download') }}
      </Button>
      <p
        v-if="running"
        class="text-sm text-muted"
      >
        {{ t('admin.geoip.running') }}
      </p>
    </template>
  </SettingsSection>
</template>
