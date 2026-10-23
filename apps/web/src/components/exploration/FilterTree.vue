<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import type { Condition, Group } from '~/generated'
import { conditionCountry } from '~/lib/client-looks'
import { useI18n } from '~/plugins/i18n'

import FilterChip from './FilterChip.vue'

/**
 * The filter as chips. Root entries are joined by separator buttons that read the root's
 * operator; pressing one groups the two entries around it under the other operator. A nested
 * group is a bordered cluster that names its operator and can be dissolved again.
 */
const props = defineProps<{
  root: Group
  fieldLabel: (condition: Condition) => string
  summary: (condition: Condition) => string
  /** Server refusals by predicate ID. */
  refused: Record<string, string>
  /** The condition being edited, hidden while its value is rebuilt. */
  editing: string | null
}>()

const emit = defineEmits<{
  edit: [condition: Condition]
  remove: [condition: Condition]
  group: [index: number]
  ungroup: [index: number]
  toggle: []
}>()

const { t } = useI18n()

const opLabel = (group: Group) => t(group.op === 'and' ? 'explore.filters.and' : 'explore.filters.or')
const conditionsOf = (group: Group): Condition[] =>
  group.items.flatMap((item) => (item.kind === 'cond' ? [item] : conditionsOf(item)))
</script>

<template>
  <button
    v-if="props.root.items.length > 1"
    type="button"
    class="h-6 shrink-0 cursor-pointer rounded-md px-1.5 text-xs font-semibold text-muted uppercase transition-colors hover:bg-secondary hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    :aria-label="t('explore.filters.rootOp', { op: opLabel(props.root) })"
    @click="emit('toggle')"
  >
    {{ t(props.root.op === 'and' ? 'explore.filters.matchAll' : 'explore.filters.matchAny') }}
  </button>
  <template
    v-for="(item, index) in props.root.items"
    :key="item.kind === 'cond' ? item.id : `g${index}`"
  >
    <button
      v-if="index > 0"
      type="button"
      class="h-6 shrink-0 cursor-pointer rounded-md px-1 text-xs text-muted transition-colors hover:bg-secondary hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      :aria-label="`${opLabel(props.root)}. ${t('explore.filters.groupWithPrevious')}`"
      :title="t('explore.filters.groupWithPrevious')"
      @click="emit('group', index)"
    >
      {{ opLabel(props.root) }}
    </button>
    <FilterChip
      v-if="item.kind === 'cond' && item.id !== props.editing"
      :field="props.fieldLabel(item)"
      :summary="props.summary(item)"
      :invalid="props.refused[item.id]"
      :country="conditionCountry(item)"
      @edit="emit('edit', item)"
      @remove="emit('remove', item)"
    />
    <span
      v-else-if="item.kind === 'group'"
      role="group"
      :aria-label="t('explore.filters.groupLabel', { count: conditionsOf(item).length, op: opLabel(item) })"
      class="inline-flex max-w-full flex-wrap items-center gap-1 rounded-lg border border-dashed border-border-strong px-1 py-0.5"
    >
      <template
        v-for="(condition, position) in conditionsOf(item)"
        :key="condition.id"
      >
        <span
          v-if="position > 0"
          class="text-xs text-muted"
        >
          {{ opLabel(item) }}
        </span>
        <FilterChip
          v-if="condition.id !== props.editing"
          :field="props.fieldLabel(condition)"
          :summary="props.summary(condition)"
          :invalid="props.refused[condition.id]"
          :country="conditionCountry(condition)"
          @edit="emit('edit', condition)"
          @remove="emit('remove', condition)"
        />
      </template>
      <button
        type="button"
        class="flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-full text-muted transition-colors hover:bg-secondary hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        :aria-label="t('explore.filters.ungroup')"
        :title="t('explore.filters.ungroup')"
        @click="emit('ungroup', index)"
      >
        <Icon
          name="lucide:ungroup"
          size="12"
        />
      </button>
    </span>
  </template>
</template>
