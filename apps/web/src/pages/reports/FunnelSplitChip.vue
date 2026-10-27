<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import DimensionMenu from '~/components/exploration/DimensionMenu.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { FilterField } from '~/composables/useFilterFields'
import type { Field } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * The "Split by" control of a funnel: a chip with the field and a remove button, or a button
 * that opens Explore's field menu. Every field but the performance metric can split a funnel.
 */
const props = defineProps<{ field: Field | undefined; fields: FilterField[] }>()
const emit = defineEmits<{ pick: [field: Field | null] }>()

const { t } = useI18n()
const format = useExplorationFormat()
const choices = computed(() => props.fields.map((f) => f.field).filter((field) => field !== 'perf_metric'))
</script>

<template>
  <div class="flex items-center gap-2">
    <span
      v-if="field"
      class="text-sm text-muted"
    >
      {{ t('reports.funnel.split.label') }}
    </span>
    <Badge
      v-if="field"
      variant="secondary"
      size="sm"
      class="gap-1"
    >
      <span class="max-w-48 truncate">{{ format.fieldLabel(field) }}</span>
      <button
        type="button"
        class="-mr-1 inline-flex cursor-pointer rounded-sm hover:bg-current/10 focus-visible:outline-2 focus-visible:outline-ring"
        :aria-label="t('reports.funnel.split.remove', { field: format.fieldLabel(field) })"
        @click="emit('pick', null)"
      >
        <Icon
          name="lucide:x"
          size="12"
        />
      </button>
    </Badge>
    <DimensionMenu
      v-else
      :fields="choices"
      @pick="(picked) => emit('pick', picked)"
    >
      <Button
        variant="ghost"
        size="xs"
      >
        <Icon
          name="lucide:plus"
          aria-hidden="true"
        />
        {{ t('reports.funnel.split.label') }}
      </Button>
    </DimensionMenu>
  </div>
</template>
