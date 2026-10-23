<script setup lang="ts">
import { toast } from 'vue-sonner'

import { retentionResource } from '~/api/resources/retention'
import SettingsSection from '~/components/account/SettingsSection.vue'
import RetentionSelect from '~/components/retention/RetentionSelect.vue'
import { Button } from '~/components/ui/button'
import { Label } from '~/components/ui/label'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useReauth } from '~/composables/useReauth'
import { useConfirmRemoval, useRetentionSettings, useUpdateDefaultRetention } from '~/composables/useRetention'
import type { Retention, RetentionDefaultArgs } from '~/generated'
import { isShorter } from '~/lib/retention'
import { useI18n } from '~/plugins/i18n'

/**
 * The installation's default event retention (decision 0016), which every site without its own
 * value follows. Shortening shows what it would remove across those sites, asks for
 * confirmation and a fresh password check.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const reauth = useReauth()
const confirmRemoval = useConfirmRemoval()
const settings = useRetentionSettings()
const update = useUpdateDefaultRetention()

const current = computed(() => settings.data.value?.installation_default)
const canManage = computed(() => settings.data.value?.can_manage_installation ?? false)
const choice = ref<Retention>('1y')
watch(current, (value) => value && (choice.value = value), { immediate: true })
const dirty = computed(() => choice.value !== current.value)
const busy = ref(false)

const save = async () => {
  if (!current.value) return
  const args: RetentionDefaultArgs = { retention: choice.value }
  busy.value = true
  try {
    if (isShorter(choice.value, current.value)) {
      const preview = await retentionResource.previewDefault(args)
      if (!(await confirmRemoval(preview))) return
    }
    const result = await reauth.run('retention.updateDefault', args, () => update.mutateAsync(args))
    if (result) toast.success(t('dataRetention.default.saved'))
  } catch (error) {
    toast.error(message(error))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SettingsSection
    id="default-retention"
    :title="t('dataRetention.default.title')"
    :description="t('dataRetention.default.description')"
  >
    <p
      v-if="settings.isError.value"
      class="text-sm text-destructive"
    >
      {{ message(settings.error.value) }}
    </p>
    <form
      v-else-if="current"
      class="grid gap-3"
      @submit.prevent="save()"
    >
      <div class="grid gap-1.5">
        <Label for="default-retention-select">{{ t('dataRetention.default.label') }}</Label>
        <RetentionSelect
          id="default-retention-select"
          v-model="choice"
          :disabled="!canManage || busy"
        />
        <p class="text-xs text-muted">{{ canManage ? t('dataRetention.otherLimits') : t('dataRetention.default.readOnly') }}</p>
      </div>
      <div v-if="canManage">
        <Button
          type="submit"
          variant="accent"
          :loading="busy"
          :disabled="!dirty"
        >
          {{ t('settings.save') }}
        </Button>
      </div>
    </form>
  </SettingsSection>
</template>
