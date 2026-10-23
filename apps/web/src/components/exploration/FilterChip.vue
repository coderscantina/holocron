<script setup lang="ts">
// Adapted from Kit (f447e797, MIT): resources/js/components/ui/data-table/FilterChip.vue.
import CountryLabel from '~/components/CountryLabel.vue'
import { SplitBadge } from '~/components/ui/badge'
import { useI18n } from '~/plugins/i18n'

/**
 * One condition as a two-part badge: the field, then how it matches. Clicking it reopens the
 * value step, so a condition is edited where it is shown. `invalid` marks a condition the
 * server refused, with the reason in the accessible name.
 */
const props = defineProps<{
  field: string
  summary: string
  invalid?: string | null
  /** ISO country code the condition names, shown as flag and name after the summary. */
  country?: string | null
}>()

const emit = defineEmits<{
  edit: []
  remove: []
}>()

const { t } = useI18n()
</script>

<template>
  <SplitBadge
    size="sm"
    :variant="props.invalid ? 'destructive' : 'secondary'"
    label-variant="surface"
    :label="field"
    removable
    :remove-label="t('explore.filters.remove', { field })"
    role="button"
    tabindex="0"
    class="max-w-full cursor-pointer focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    value-class="min-w-0"
    :aria-label="props.invalid ? `${t('explore.filters.edit', { field })}. ${props.invalid}` : t('explore.filters.edit', { field })"
    :title="props.invalid ?? undefined"
    @remove="emit('remove')"
    @click="emit('edit')"
    @keydown.enter.prevent="emit('edit')"
    @keydown.space.prevent="emit('edit')"
  >
    <span
      class="min-w-0"
      :class="country ? 'shrink-0' : 'truncate'"
    >{{ summary }}</span>
    <CountryLabel
      v-if="country"
      :code="country"
      class="ml-1"
    />
  </SplitBadge>
</template>
