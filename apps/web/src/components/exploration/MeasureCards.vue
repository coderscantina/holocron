<script setup lang="ts">
import { Skeleton } from '~/components/ui/skeleton'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { Cut, Measure, VisitorLabel } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import ChangeBadge from './ChangeBadge.vue'

/**
 * The measures of a cut as one segmented strip: a filled track with a cell per measure, each
 * with its label, the exact value and, when compared, the change in words and an arrow. With
 * `selected` the cells are a choice: the selected one takes the canvas colour like an active segment, and
 * picking a cell emits `select`. Cells stay in one row from 640px; phones get two columns
 * inside the same track. A value too long for its cell shrinks to fit instead of being cut off.
 */
const props = defineProps<{
  measures: Measure[]
  cut: Cut | null
  visitorLabel: VisitorLabel | null
  /** Index of the selected measure; omit for a strip that only shows values. */
  selected?: number
}>()

const emit = defineEmits<{ select: [index: number] }>()

const { t } = useI18n()
const format = useExplorationFormat()

/** Visitors and visitor-days are the same count; the label follows the range (0003). */
const label = (measure: Measure) =>
  (measure === 'visitor_days' || measure === 'visitors') && props.visitorLabel
    ? format.measureLabel(props.visitorLabel)
    : format.measureLabel(measure)

const selectable = computed(() => props.selected !== undefined)

/**
 * Shrinks a long value, like an amount with its currency, to the width of its cell: about 0.6em
 * per character, never above the strip's own size. Short values keep the full size.
 */
const fit = (text: string) => ({ fontSize: `min(1em, calc(100cqi / ${Math.max(text.length, 1) * 0.6}))` })
</script>

<template>
  <section
    :aria-label="t('overview.measures')"
    class="grid grid-cols-2 gap-1 rounded-panel bg-fill p-1 sm:auto-cols-fr sm:grid-flow-col sm:grid-cols-none"
  >
    <component
      :is="selectable ? 'button' : 'div'"
      v-for="(measure, index) in measures"
      :key="index"
      :type="selectable ? 'button' : undefined"
      :aria-pressed="selectable ? index === selected : undefined"
      class="@container min-w-0 rounded-control px-4 pt-3.5 pb-4 text-left transition-colors sm:px-5"
      :class="[
        selectable &&
          'cursor-pointer hover:bg-fill-strong focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring',
        index === selected && 'bg-surface shadow-soft-sm ring-1 ring-hairline ring-inset hover:bg-surface',
      ]"
      @click="selectable && emit('select', index)"
    >
      <span
        class="block truncate text-sm text-muted"
        :class="index === selected && 'text-foreground'"
      >
        {{ label(measure) }}
      </span>
      <template v-if="cut">
        <span
          class="mt-2 block truncate text-[2rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums sm:text-[2.375rem]"
          :title="format.metric(cut.totals[index])"
        >
          <span :style="fit(format.metric(cut.totals[index]))">{{ format.metric(cut.totals[index]) }}</span>
        </span>
        <span
          v-if="cut.compare?.[index]"
          class="mt-2.5 block truncate text-sm"
        >
          <ChangeBadge
            :comparison="cut.compare[index]"
            :measure="measure"
            words
          />
        </span>
      </template>
      <template v-else>
        <Skeleton class="mt-2 h-8 w-24 sm:h-[2.375rem]" />
        <Skeleton class="mt-2.5 h-4 w-16" />
      </template>
    </component>
  </section>
</template>
