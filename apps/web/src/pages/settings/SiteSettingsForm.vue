<script setup lang="ts">
import SettingsPage from '~/components/account/SettingsPage.vue'
import SettingsSection from '~/components/account/SettingsSection.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { useCommandForm } from '~/composables/useCommandForm'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useSite, useUpdateSite } from '~/composables/useSiteContext'
import type { Site } from '~/generated'
import { SITE_NAME_MAX_CHARS } from '~/generated/limits'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import SiteManageSection from '~/pages/settings/SiteManageSection.vue'
import { useI18n } from '~/plugins/i18n'

const props = defineProps<{ site: Site; revision: number }>()

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()
const update = useUpdateSite()
const siteQuery = useSite(() => props.site.id)

const name = ref(props.site.name)
const domain = ref(props.site.domain)

const canConfigure = computed(() => props.site.capabilities.includes('site.configure'))
const dirty = computed(() => name.value !== props.site.name || domain.value !== props.site.domain)

const error = computed(() => update.error.value)
const { saved, fieldErrors, submit } = useCommandForm([name, domain], () => error.value)

// A fresh snapshot (after a save or a reload) becomes the form's starting point.
watch(
  () => props.site,
  (site) => {
    name.value = site.name
    domain.value = site.domain
  }
)

const save = () =>
  submit((idempotencyKey) =>
    update.mutateAsync({
      args: { site: props.site.id, name: name.value, domain: domain.value, expected_revision: props.revision },
      idempotencyKey,
    })
  )

const reload = async () => {
  update.reset()
  await siteQuery.refetch()
}
</script>

<template>
  <SettingsPage>
    <SettingsSection
      id="site"
      :title="t('settings.site')"
      :description="canConfigure ? t('settings.generalDescription') : t('settings.readOnly')"
    >
      <form
        id="site-form"
        class="grid gap-4"
        novalidate
        @submit.prevent="save().catch(() => {})"
      >
        <FormField
          id="site-name"
          v-model="name"
          :label="t('settings.name')"
          :description="t('settings.nameHint', { max: SITE_NAME_MAX_CHARS })"
          :error="fieldErrors ? fieldMessage(error, 'name') : null"
          :readonly="!canConfigure"
          required
        />
        <FormField
          id="site-domain"
          v-model="domain"
          :label="t('settings.domain')"
          :description="t('settings.domainHint')"
          :error="fieldErrors ? fieldMessage(error, 'domain') : null"
          :readonly="!canConfigure"
          autocomplete="off"
          required
        />
        <CommandAlert
          :error="error"
          @reload="reload()"
        />
      </form>
      <template
        v-if="canConfigure"
        #footer
      >
        <Button
          type="submit"
          form="site-form"
          variant="accent"
          :loading="update.isPending.value"
          :disabled="!dirty && !update.isError.value"
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
    <SiteManageSection :site="site" />
  </SettingsPage>
</template>
