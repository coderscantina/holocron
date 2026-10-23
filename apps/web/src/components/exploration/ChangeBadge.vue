<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { Comparison, Measure } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { changeTone, changeWords } from '~/services/exploration'

/**
 * The change against the comparison. Direction is carried by an arrow and words, not by
 * colour alone; a zero baseline reads "new" instead of an invented percentage. `words` reads
 * it as the measures strip does: "12 % above", "about the same" within ±5 %. With `measure`
 * the change takes the success or destructive tone by whether it is good news for that measure,
 * and a change within ±5 % goes muted.
 */
const props = defineProps<{ comparison: Comparison; words?: boolean; measure?: Measure }>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()

const described = computed(() => {
  const words = changeWords(props.comparison)
  switch (words?.kind) {
    case 'above':
      return t('explore.change.above', { percent: numbers.percent(words.ratio) })
    case 'below':
      return t('explore.change.below', { percent: numbers.percent(words.ratio) })
    case 'same':
      return t('explore.change.same')
    case 'new':
      return t('explore.change.new')
    default:
      return '–'
  }
})

const tones = { good: 'text-success', bad: 'text-destructive', neutral: 'text-muted' } as const
const tone = computed(() => (props.measure === undefined ? 'text-foreground' : tones[changeTone(props.measure, props.comparison)]))

const direction = computed(() => {
  if (props.words && changeWords(props.comparison)?.kind === 'same') return 'flat'
  const change = props.comparison.change ?? 0
  return change > 0 ? 'up' : change < 0 ? 'down' : 'flat'
})
</script>

<template>
  <span
    class="inline-flex items-center gap-0.5 font-medium whitespace-nowrap tabular-nums"
    :class="tone"
  >
    <Icon
      v-if="direction !== 'flat'"
      :name="direction === 'up' ? 'lucide:arrow-up-right' : 'lucide:arrow-down-right'"
      size="1.1em"
      aria-hidden="true"
    />
    <span
      v-if="direction !== 'flat' && !words"
      class="sr-only"
    >
      {{ direction === 'up' ? t('explore.compare.up') : t('explore.compare.down') }}
    </span>
    <template v-if="words">{{ described }}</template>
    <template v-else-if="props.comparison.relative_change !== undefined">
      {{ format.change(props.comparison.relative_change) }}
    </template>
    <template v-else-if="props.comparison.reason === 'zero_baseline' && (props.comparison.change ?? 0) !== 0">
      {{ t('explore.compare.new') }}
    </template>
    <template v-else-if="props.comparison.change === 0">
      {{ t('explore.compare.noChange') }}
    </template>
    <template v-else>–</template>
  </span>
</template>
