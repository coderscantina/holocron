<script setup lang="ts">
import { toast } from 'vue-sonner'

import { sitesResource } from '~/api/resources/sites'
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import RetentionSelect from '~/components/retention/RetentionSelect.vue'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useConfirmRemoval, useRetentionSettings, useUpdateSiteRetention } from '~/composables/useRetention'
import { useSite } from '~/composables/useSiteContext'
import type { Retention, Site, SitesUpdateRetentionArgs } from '~/generated'
import { isShorter, retentionLabels } from '~/lib/retention'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'

/**
 * How long this site keeps its events (decision 0016). Shortening shows what it would remove,
 * asks for confirmation and a fresh password check; lengthening saves directly.
 */
const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const reauth = useReauth()
const confirmRemoval = useConfirmRemoval()
const settings = useRetentionSettings()
const update = useUpdateSiteRetention()
const siteQuery = useSite(() => props.site.id)

const canChange = computed(() => props.site.capabilities.includes('site.delete'))
const current = computed<Retention | 'default'>(() => props.site.retention_override ?? 'default')
const choice = ref(current.value)
watch(current, (value) => (choice.value = value))

const installationDefault = computed(() => settings.data.value?.installation_default)
const defaultLabel = computed(() =>
  installationDefault.value
    ? t('dataRetention.site.followDefault', { value: t(retentionLabels[installationDefault.value]) })
    : t('dataRetention.site.followDefaultPlain')
)
const effective = computed(() => (choice.value === 'default' ? installationDefault.value : choice.value))
const dirty = computed(() => choice.value !== current.value)

const busy = ref(false)
const saved = ref(false)
watch(choice, () => (saved.value = false))

const save = async () => {
  const retention = effective.value
  if (!retention) return
  const args: SitesUpdateRetentionArgs = {
    site: props.site.id,
    ...(choice.value === 'default' ? {} : { retention: choice.value }),
    expected_revision: props.revision,
  }
  busy.value = true
  update.reset()
  try {
    if (isShorter(retention, props.site.retention)) {
      const preview = await sitesResource.retentionPreview(args).catch((error: unknown) => {
        toast.error(message(error))
      })
      if (!preview || !(await confirmRemoval(preview, props.site.timezone))) return
    }
    const result = await reauth.run('sites.updateRetention', args, () => update.mutateAsync(args)).catch(() => null)
    if (result) saved.value = true
  } finally {
    busy.value = false
  }
}

const reload = async () => {
  update.reset()
  await siteQuery.refetch()
}
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="retention"
      :title="t('dataRetention.title')"
      :description="canChange ? t('dataRetention.site.description') : t('settings.readOnly')"
    >
      <form
        id="retention-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save()"
      >
        <div class="grid gap-1.5">
          <Label for="site-retention">{{ t('dataRetention.site.label') }}</Label>
          <RetentionSelect
            id="site-retention"
            v-model="choice"
            :default-label="defaultLabel"
            :disabled="!canChange || busy"
          />
          <p class="text-xs text-muted">
            {{ t('dataRetention.site.current', { value: t(retentionLabels[site.retention]) }) }}
            {{ t('dataRetention.otherLimits') }}
          </p>
        </div>

        <CommandAlert
          :error="update.error.value"
          @reload="reload()"
        />
      </form>
      <template
        v-if="canChange"
        #footer
      >
        <Button
          type="submit"
          form="retention-form"
          variant="accent"
          :loading="busy"
          :disabled="!dirty || !effective"
        >
          {{ t('settings.save') }}
        </Button>
        <p
          class="text-sm text-muted"
          role="status"
        >
          {{ saved && !dirty ? t('settings.saved') : '' }}
        </p>
      </template>
    </SettingsSection>
  </SettingsPage>
</template>
