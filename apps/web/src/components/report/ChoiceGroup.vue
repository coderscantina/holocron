<script setup lang="ts" generic="Value extends string | number">
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'

/**
 * A labelled single choice drawn as segments: a radio group with one tab stop, arrow keys to
 * move between options, and the checked option exposed to assistive technology.
 */
const props = defineProps<{
  label: string
  options: { value: Value; label: string }[]
}>()

const model = defineModel<Value>({ required: true })

const buttons = ref<HTMLButtonElement[]>([])

const move = (index: number, delta: number) => {
  const count = props.options.length
  const next = (index + delta + count) % count
  const option = props.options[next]
  if (!option) return
  model.value = option.value
  void nextTick(() => buttons.value[next]?.focus())
}
</script>

<template>
  <div
    role="radiogroup"
    :aria-label="label"
    :class="segmentedTrack"
  >
    <button
      v-for="(option, index) in options"
      :key="option.value"
      ref="buttons"
      type="button"
      role="radio"
      :aria-checked="model === option.value"
      :tabindex="model === option.value || (index === 0 && !options.some((o) => o.value === model)) ? 0 : -1"
      :class="segmentedItem()"
      @click="model = option.value"
      @keydown.right.prevent="move(index, 1)"
      @keydown.down.prevent="move(index, 1)"
      @keydown.left.prevent="move(index, -1)"
      @keydown.up.prevent="move(index, -1)"
    >
      {{ option.label }}
    </button>
  </div>
</template>
