<script setup lang="ts">
import { useMutation } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { sharingResource } from '~/api/resources/sharing'
import SecretReveal from '~/components/account/SecretReveal.vue'
import CheckboxGroup from '~/components/CheckboxGroup.vue'
import FormField from '~/components/FormField.vue'
import { Button } from '~/components/ui/button'
import { Dialog, DialogContent, DialogHeaderCombined } from '~/components/ui/dialog'
import { Label } from '~/components/ui/label'
import { Switch } from '~/components/ui/switch'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { DimensionField, NamedMeasure, ShareCreated, SharingCreateArgs, Site } from '~/generated'
import { LABEL_MAX_CHARS, SHARE_LIFETIME_MAX_DAYS } from '~/generated/limits'
import { dimensionFields, fieldLabels, measureLabels, namedMeasures } from '~/lib/exploration-labels'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { useI18n } from '~/plugins/i18n'
import { queryClient } from '~/plugins/vue-query'
import { shareArgs } from '~/services/sharing'

/**
 * A restricted share of the site's analytics: which measures and fields viewers may use, how
 * far back they may read, whether they may export or watch recordings, and when it ends. The
 * viewer link is shown once.
 */
const props = defineProps<{ site: Site }>()
const open = defineModel<boolean>('open', { default: false })

const { t } = useI18n()
const { fieldMessage } = useErrorMessage()

const DEFAULT_RANGE = 30
const DEFAULT_LIFETIME = 30
const MAX_RANGE = 366

const name = ref('')
const measures = ref<NamedMeasure[]>(['pageviews', 'visitor_days'])
const dimensions = ref<DimensionField[]>(['path'])
const rangeMaxDays = ref(String(DEFAULT_RANGE))
const lifetimeDays = ref(String(DEFAULT_LIFETIME))
const narrowing = ref(false)
const exportGrant = ref(false)
const recordingsGrant = ref(false)
const created = ref<ShareCreated | null>(null)

const create = useMutation({
  mutationFn: (args: SharingCreateArgs) => sharingResource.create(args),
  onSuccess: (result) => {
    created.value = result
    void queryClient.invalidateQueries({ queryKey: queryKeys.shares(props.site.id) })
  },
})

watch(open, (value) => {
  if (value) return
  name.value = ''
  measures.value = ['pageviews', 'visitor_days']
  dimensions.value = ['path']
  rangeMaxDays.value = String(DEFAULT_RANGE)
  lifetimeDays.value = String(DEFAULT_LIFETIME)
  narrowing.value = false
  exportGrant.value = false
  recordingsGrant.value = false
  created.value = null
  create.reset()
})

const days = (text: string, max: number) => {
  const value = Number.parseInt(text, 10)
  return Number.isInteger(value) && value >= 1 && value <= max ? value : null
}
const range = computed(() => days(rangeMaxDays.value, MAX_RANGE))
const lifetime = computed(() => days(lifetimeDays.value, SHARE_LIFETIME_MAX_DAYS))
const ready = computed(() => name.value.trim() !== '' && measures.value.length > 0 && range.value !== null && lifetime.value !== null)

const submit = () => {
  if (range.value === null || lifetime.value === null) return
  create.mutate(
    shareArgs(props.site, {
      name: name.value,
      measures: measures.value,
      dimensions: dimensions.value,
      rangeMaxDays: range.value,
      narrowing: narrowing.value,
      exportGrant: exportGrant.value,
      recordingsGrant: recordingsGrant.value,
      lifetimeDays: lifetime.value,
    })
  )
}

const link = computed(() =>
  created.value?.secret ? `${window.location.origin}/share/${encodeURIComponent(created.value.secret)}` : null
)
const measureOptions = namedMeasures.map((value) => ({ value, label: t(measureLabels[value]) }))
const dimensionOptions = dimensionFields.map((value) => ({ value, label: t(fieldLabels[value]) }))
const canRecordings = computed(() => props.site.capabilities.includes('site.recordings.read'))
const canExport = computed(() => props.site.capabilities.includes('site.export'))
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
          :title="t('sharing.createdTitle')"
          :description="t('sharing.createdDescription')"
        />
        <SecretReveal
          id="share-link"
          :label="t('sharing.link')"
          :secret="link"
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
          :title="t('sharing.create')"
          :description="t('sharing.createDescription', { site: site.name })"
        />
        <form
          class="grid gap-5"
          novalidate
          @submit.prevent="submit"
        >
          <FormField
            id="share-name"
            v-model="name"
            autocomplete="off"
            :label="t('sharing.name')"
            :description="t('sharing.nameHint', { max: LABEL_MAX_CHARS })"
            :error="fieldMessage(error, 'name')"
            required
          />
          <CheckboxGroup
            v-model="measures"
            :legend="t('sharing.measures')"
            :options="measureOptions"
            :error="fieldMessage(error, 'target.limits.measures')"
          />
          <CheckboxGroup
            v-model="dimensions"
            :legend="t('sharing.dimensions')"
            :options="dimensionOptions"
            :error="fieldMessage(error, 'target.limits.dimensions')"
          />
          <div class="grid gap-4 sm:grid-cols-2">
            <FormField
              id="share-range"
              v-model="rangeMaxDays"
              type="number"
              inputmode="numeric"
              :label="t('sharing.rangeMax')"
              :description="t('sharing.rangeMaxHint', { max: MAX_RANGE })"
              :error="fieldMessage(error, 'target.limits.range_max_days')"
              required
            />
            <FormField
              id="share-lifetime"
              v-model="lifetimeDays"
              type="number"
              inputmode="numeric"
              :label="t('sharing.lifetime')"
              :description="t('sharing.lifetimeHint', { max: SHARE_LIFETIME_MAX_DAYS })"
              :error="fieldMessage(error, 'expires_at')"
              required
            />
          </div>
          <fieldset class="grid gap-3">
            <legend class="mb-1 text-sm font-medium text-primary">{{ t('sharing.actions') }}</legend>
            <div class="flex items-center gap-3">
              <Switch
                id="share-narrowing"
                v-model="narrowing"
              />
              <Label for="share-narrowing">{{ t('sharing.narrowing') }}</Label>
            </div>
            <div class="flex items-center gap-3">
              <Switch
                id="share-export"
                v-model="exportGrant"
                :disabled="!canExport"
              />
              <Label for="share-export">{{ t('sharing.export') }}</Label>
            </div>
            <div class="flex items-center gap-3">
              <Switch
                id="share-recordings"
                v-model="recordingsGrant"
                :disabled="!canRecordings"
              />
              <Label for="share-recordings">{{ t('sharing.recordings') }}</Label>
            </div>
          </fieldset>
          <CommandAlert
          :shown="['name', 'target.limits.measures', 'target.limits.dimensions', 'target.limits.range_max_days', 'expires_at']" :error="error" />
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
              {{ t('sharing.create') }}
            </Button>
          </div>
        </form>
      </template>
    </DialogContent>
  </Dialog>
</template>
