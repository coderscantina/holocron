<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import { Popover, PopoverAnchor, PopoverContent } from '~/components/ui/popover'
import { useFormat } from '~/composables/useFormat'
import { useStepDuration } from '~/composables/useStepDuration'
import type { StepDuration } from '~/generated'
import { DURATION_BUCKETS } from '~/lib/funnel-duration'
import { useI18n } from '~/plugins/i18n'

/**
 * The median time from one step to the next, as a small button in the ribbon. Hover, focus or a
 * tap opens the spread: p25, median, p75 and the five bucket counts. A step nobody reached has
 * no duration, so nothing renders.
 */
const props = defineProps<{
  duration: StepDuration
  /** Where the median reads: "median 2 min 39 s" between columns, or "2 min 39 s after step 1" under a label. */
  placement: 'gap' | 'label'
}>()

const { t } = useI18n()
const numbers = useFormat()
const text = useStepDuration()

const median = computed(() => text.metric(props.duration.median))
const total = computed(() => props.duration.buckets.reduce((sum, count) => sum + count, 0))
const peak = computed(() => Math.max(1, ...props.duration.buckets))

const open = ref(false)
/** A mouse opens on hover and closes on leave; touch and pen toggle on tap; the keyboard opens on focus. */
const pointer = ref<string>('mouse')
const onFocus = (event: FocusEvent) => {
  if (event.target instanceof HTMLElement && event.target.matches(':focus-visible')) open.value = true
}
const onClick = () => {
  open.value = pointer.value === 'mouse' ? true : !open.value
}
</script>

<template>
  <Popover
    v-if="median"
    :open="open"
    @update:open="(value) => (open = value)"
  >
    <PopoverAnchor as-child>
      <button
        type="button"
        class="flex h-6 cursor-pointer items-center gap-1.5 rounded-control bg-fill px-2 text-xs whitespace-nowrap text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        :aria-label="t('reports.funnel.duration.gap', { time: median, from: duration.from_step + 1, to: duration.to_step + 1 })"
        :aria-expanded="open"
        @pointerdown="(event) => (pointer = event.pointerType)"
        @pointerenter="(event) => event.pointerType === 'mouse' && (open = true)"
        @pointerleave="(event) => event.pointerType === 'mouse' && (open = false)"
        @focus="onFocus"
        @blur="open = false"
        @click="onClick"
      >
        <Icon
          name="lucide:clock"
          size="12"
          aria-hidden="true"
        />
        <span class="text-primary tabular-nums">
          {{
            placement === 'gap'
              ? t('reports.funnel.duration.median', { time: median })
              : t('reports.funnel.duration.after', { time: median, step: duration.from_step + 1 })
          }}
        </span>
      </button>
    </PopoverAnchor>
    <PopoverContent
      side="top"
      align="center"
      :side-offset="6"
      :collision-padding="8"
      class="pointer-events-none w-[26rem] max-w-[calc(100vw-16px)] p-3"
      @open-auto-focus.prevent
      @close-auto-focus.prevent
    >
      <p class="text-sm font-semibold text-primary tabular-nums">
        {{ t('reports.funnel.duration.popTitle', { from: duration.from_step + 1, to: duration.to_step + 1, count: numbers.integer(total) }) }}
      </p>
      <dl class="mt-2 grid grid-cols-3 gap-2 text-xs text-muted">
        <div>
          <dt>{{ t('reports.funnel.duration.p25') }}</dt>
          <dd class="text-sm font-semibold text-primary tabular-nums">{{ text.metric(duration.p25) }}</dd>
        </div>
        <div>
          <dt>{{ t('reports.funnel.duration.p50') }}</dt>
          <dd class="text-sm font-semibold text-primary tabular-nums">{{ median }}</dd>
        </div>
        <div>
          <dt>{{ t('reports.funnel.duration.p75') }}</dt>
          <dd class="text-sm font-semibold text-primary tabular-nums">{{ text.metric(duration.p75) }}</dd>
        </div>
      </dl>
      <ul class="mt-3 grid grid-cols-5 gap-2 text-xs">
        <li
          v-for="(count, index) in duration.buckets"
          :key="index"
          class="grid content-start gap-1"
        >
          <span
            class="tabular-nums"
            :class="count > 0 ? 'text-primary' : 'text-muted'"
          >
            {{ numbers.integer(count) }}
          </span>
          <span
            class="h-1.5 rounded-full bg-fill-strong"
            aria-hidden="true"
          >
            <span
              class="block h-full rounded-full bg-accent"
              :style="{ width: `${(count / peak) * 100}%` }"
            />
          </span>
          <span class="leading-tight text-muted">{{ t(`reports.funnel.duration.buckets.${DURATION_BUCKETS[index]!}`) }}</span>
        </li>
      </ul>
    </PopoverContent>
  </Popover>
</template>
