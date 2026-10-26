<script setup lang="ts">
import { useIntervalFn, useMediaQuery, useNow } from '@vueuse/core'
import { PopoverContent } from 'reka-ui'

import TopBarScope from '~/components/app/TopBarScope.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { Popover, PopoverAnchor } from '~/components/ui/popover'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import type { FilterField } from '~/composables/useFilterFields'
import { valueText } from '~/composables/useFilterFields'
import type { Compare, Condition, ExplorationState, Node, ResolvedRange, Retention } from '~/generated'
import { operatorLabels, weekdayLabels } from '~/lib/exploration-labels'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import {
  baselineOf,
  canonical,
  compareBlock,
  containsNow,
  fitRange,
  setCompare,
  weekdayOf,
  type Baseline,
  type CompareBlock,
} from '~/services/exploration'

import FilterBar from './FilterBar.vue'
import RangeMenu from './RangeMenu.vue'
import SegmentsMenu from './SegmentsMenu.vue'

/**
 * The scope chips of one state in the top bar: range ("until now" while the range contains
 * now), comparison, one chip per filter entry, and "Filter or segment". Filter and
 * segment chips open the staged filter editor in one popover anchored to the chips. Each change
 * is one new state the page commits. Below 448px the bar shows an icon-only summary chip, so the
 * summary also reads as a line on the canvas.
 */
const props = defineProps<{
  state: ExplorationState
  /** Bounds of the shown result, when it is the state's own. */
  resolved: ResolvedRange | null
  /** The state's typical-weekday baseline has fewer than the days it needs. */
  insufficient: boolean
  fields: FilterField[]
  refused: Record<string, string>
  canSaveSegments: boolean
  /** The site's event retention: it decides which long presets the range menu offers. */
  retention?: Retention
}>()

const emit = defineEmits<{
  commit: [state: ExplorationState]
  notice: [message: string, tone: 'success' | 'error']
}>()

const { t, locale } = useI18n()
const format = useExplorationFormat()
const now = useNow({ scheduler: (update) => useIntervalFn(update, 30_000) })

/**
 * The bounds the chips read: the shown result's, or while a new result loads, the last ones
 * resolved for the same range and timezone. A new filter, interval or refresh on a custom range
 * then keeps its chips as they are instead of shrinking them and shifting every chip beside them.
 */
const scope = computed(() => canonical([props.state.range, props.state.timezone]))
const known = shallowRef<{ scope: string; resolved: ResolvedRange } | null>(null)
watchEffect(() => {
  if (props.resolved) known.value = { scope: scope.value, resolved: props.resolved }
})
const bounds = computed(() => props.resolved ?? (known.value?.scope === scope.value ? known.value.resolved : null))

/** Relative ranges and all time always end with today, so they say so before their result arrives. */
const until = computed(() => {
  const { range, timezone } = props.state
  const resolved = bounds.value
  const live = range.kind !== 'days' || (resolved !== null && containsNow(resolved, now.value.getTime()))
  if (!live) return null
  const time = new Intl.DateTimeFormat(locale.value, { timeZone: timezone, timeStyle: 'short' }).format(now.value)
  return t('explore.scope.untilNow', { time })
})

/** The baseline the chip names; before a result arrives, "Today" names the clock's weekday. */
const baseline = computed<Baseline | null>(() => {
  const { state } = props
  const resolved = bounds.value
  if (resolved) return baselineOf(state.compare, resolved)
  if (state.compare === 'none') return null
  if (state.compare === 'previous_period') return { kind: 'previous' }
  const today = state.range.kind === 'relative' && state.range.unit === 'day' && state.range.count === 1
  return today ? { kind: 'weekday', weekday: weekdayOf(now.value.getTime(), state.timezone) } : { kind: 'weekdays' }
})

const weekdayName = computed(() => (baseline.value?.kind === 'weekday' ? t(weekdayLabels[baseline.value.weekday]) : ''))

const compareLabel = computed(() => {
  const current = baseline.value
  if (!current) return t('explore.scope.compare.none')
  if (current.kind === 'previous') return t('explore.scope.compare.previous')
  if (props.insufficient) return t('explore.scope.compare.insufficient')
  return current.kind === 'weekday'
    ? t('explore.scope.compare.weekday', { weekday: weekdayName.value })
    : t('explore.scope.compare.weekdays')
})

const blockReason = (block: CompareBlock | null): string | null => {
  switch (block?.reason) {
    case 'range':
      return t('explore.scope.compare.blocked.range', { days: block.days })
    case 'measure':
      return t('explore.scope.compare.blocked.measure', { measure: format.measureLabel(block.measure) })
    case 'report':
      return t('explore.scope.compare.blocked.report')
    default:
      return null
  }
}

const compareOptions = computed(() => {
  const typical = weekdayName.value
    ? t('explore.scope.compare.options.weekday', { weekday: weekdayName.value })
    : t('explore.scope.compare.options.weekdays')
  const labels: Record<Compare, string> = {
    none: t('explore.scope.compare.options.none'),
    previous_period: t('explore.scope.compare.options.previous'),
    typical_weekday: typical,
  }
  const order: Compare[] = ['none', 'previous_period', 'typical_weekday']
  return order.map((compare) => ({ compare, label: labels[compare], blocked: blockReason(compareBlock(props.state, compare)) }))
})

const isCompare = (value: unknown): value is Compare => compareOptions.value.some((option) => option.compare === value)
const onCompare = (value: unknown) => {
  if (isCompare(value)) emit('commit', setCompare(props.state, value))
}

const entries = computed<Node[]>(() => props.state.filter?.items ?? [])

const operatorText = (condition: Condition) =>
  [t(operatorLabels[condition.cmp]), condition.value ? valueText(condition.value) : ''].filter(Boolean).join(' ')

const firstCondition = (node: Node): Condition | null =>
  node.kind === 'cond' ? node : node.items.reduce<Condition | null>((found, item) => found ?? firstCondition(item), null)

const chips = computed(() =>
  entries.value.map((node) => {
    const first = firstCondition(node)
    const field = first ? format.fieldLabel(first.field) : ''
    return node.kind === 'cond'
      ? { key: node.id, field, text: operatorText(node), refused: props.refused[node.id] ?? null }
      : { key: `group-${first?.id ?? ''}`, field, text: t('explore.scope.group', { count: node.items.length }), refused: null }
  })
)

const editor = ref(false)
/**
 * The chips remount when the bar collapses them into its summary popover or expands them again,
 * so the editor closes with the chips that anchor it. Its content renders beside the chips, not
 * in a portal, so editing inside the summary popover does not count as a click outside it.
 */
const anchor = useTemplateRef<HTMLElement>('anchor')
watch(anchor, () => {
  editor.value = false
})

const filterCount = computed(() => chips.value.length)
const filtersKey = computed<MessageKey | null>(() =>
  filterCount.value === 0 ? null : filterCount.value === 1 ? 'explore.scope.oneFilter' : 'explore.scope.filters'
)
/** Rendered only on a phone, so the controls leave nothing on the canvas elsewhere. */
const narrow = useMediaQuery('(width < 28rem)')

const summary = computed(() =>
  [
    format.rangeLabel(props.state.range),
    props.state.report ? null : compareLabel.value,
    filtersKey.value ? t(filtersKey.value, { count: filterCount.value }) : null,
  ]
    .filter(Boolean)
    .join(' · ')
)
</script>

<template>
  <p
    v-if="narrow"
    class="text-sm text-muted"
  >
    {{ summary }}
  </p>
  <TopBarScope :summary="summary">
    <RangeMenu
      chip
      :range="state.range"
      :until="until"
      :retention="retention"
      @change="(range) => emit('commit', fitRange(state, range))"
    />
    <DropdownMenu v-if="!state.report">
      <DropdownMenuTrigger as-child>
        <Button
          variant="fill"
          size="control"
          :aria-label="`${t('explore.scope.compare.label')}: ${compareLabel}`"
        >
          <Icon
            name="lucide:arrow-left-right"
            class="text-muted"
            aria-hidden="true"
          />
          {{ compareLabel }}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        class="w-64"
      >
        <DropdownMenuRadioGroup
          :model-value="state.compare"
          @update:model-value="onCompare"
        >
          <DropdownMenuRadioItem
            v-for="option in compareOptions"
            :key="option.compare"
            :value="option.compare"
            :disabled="option.blocked !== null && option.compare !== state.compare"
          >
            <span class="flex min-w-0 flex-col">
              <span>{{ option.label }}</span>
              <span
                v-if="option.blocked"
                class="text-xs text-muted"
              >
                {{ option.blocked }}
              </span>
            </span>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
    <Popover v-model:open="editor">
      <PopoverAnchor as-child>
        <div
          ref="anchor"
          class="flex items-center gap-2"
        >
          <Button
            v-for="chip in chips"
            :key="chip.key"
            variant="fill"
            size="control"
            class="max-w-64"
            :class="chip.refused && 'text-destructive'"
            :aria-label="t('explore.filters.edit', { field: chip.field })"
            :title="chip.refused ?? undefined"
            @click="editor = true"
          >
            <span class="text-muted">{{ chip.field }}</span>
            <span class="truncate">{{ chip.text }}</span>
          </Button>
          <Button
            variant="ghost"
            size="control"
            class="text-muted ring-1 ring-fill-strong ring-inset hover:bg-fill"
            :aria-expanded="editor"
            @click="editor = !editor"
          >
            <Icon
              name="lucide:plus"
              aria-hidden="true"
            />
            {{ t('explore.scope.addFilter') }}
          </Button>
        </div>
      </PopoverAnchor>
      <PopoverContent
        align="start"
        :side-offset="4"
        class="z-50 grid w-[min(32rem,calc(100vw-2rem))] gap-2 rounded-md border border-popover-border bg-popover p-2 text-popover-foreground shadow-soft-lg outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95"
        :aria-label="t('explore.scope.filterEditor')"
      >
        <div class="flex min-w-0 items-start gap-2">
          <FilterBar
            class="flex-1"
            :state="state"
            :fields="fields"
            :refused="refused"
            @commit="(next) => emit('commit', next)"
          />
          <SegmentsMenu
            :state="state"
            :can-save="canSaveSegments"
            @commit="(next) => emit('commit', next)"
            @notice="(message, tone) => emit('notice', message, tone)"
          />
        </div>
      </PopoverContent>
    </Popover>
  </TopBarScope>
</template>
