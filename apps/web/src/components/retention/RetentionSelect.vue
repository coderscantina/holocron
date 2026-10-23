<script setup lang="ts">
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '~/components/ui/select'
import type { Retention } from '~/generated'
import { retentionLabels, retentions } from '~/lib/retention'
import { useI18n } from '~/plugins/i18n'

/** The six retention values, plus an "installation default" choice when `defaultLabel` is given. */
defineProps<{ id: string; disabled?: boolean; defaultLabel?: string }>()
const model = defineModel<Retention | 'default'>({ required: true })

const { t } = useI18n()
</script>

<template>
  <Select
    v-model="model"
    :disabled="disabled"
  >
    <SelectTrigger :id="id">
      <SelectValue>{{ model === 'default' ? defaultLabel : t(retentionLabels[model]) }}</SelectValue>
    </SelectTrigger>
    <SelectContent>
      <SelectItem
        v-if="defaultLabel"
        value="default"
      >
        {{ defaultLabel }}
      </SelectItem>
      <SelectItem
        v-for="value in retentions"
        :key="value"
        :value="value"
      >
        {{ t(retentionLabels[value]) }}
      </SelectItem>
    </SelectContent>
  </Select>
</template>
