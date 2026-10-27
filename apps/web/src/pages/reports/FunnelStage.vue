<script setup lang="ts">
import { useElementSize, useMediaQuery } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import StageView from '~/components/stage/StageView.vue'
import { useExplore } from '~/composables/useExplore'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import { useStepDuration } from '~/composables/useStepDuration'
import type { Cell, ExplorationState, Field, FunnelResult, Group, Metric, ResolvedRange } from '~/generated'
import { previousDays, ribbon, worstLeak, type LeakKind, type RibbonFlow } from '~/lib/funnel-ribbon'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { conditions, encodeState } from '~/services/exploration'
import { funnelOf, sessionsFor, setSplit } from '~/services/reports'
import { pageOf } from '~/lib/state-pages'

import StepGap from './StepGap.vue'

/**
 * The funnel as a ribbon that narrows at each step, with the visitors who stop peeling off
 * below it by where they went next. The rail holds the conversion with its change against the
 * previous period, the worst step in one sentence, and the leak in focus: where its visitors
 * went and their latest sessions. The worst leak is in focus until another is picked. The rail
 * also holds the time to convert, and the median time between steps sits between the columns.
 * A picked `group` of a split funnel names itself above the conversion and compares with all
 * traffic; the `below` slot holds the split table.
 */
const props = defineProps<{
  state: ExplorationState
  report: FunnelResult
  resolved: ResolvedRange
  group?: { field: Field; cell: Cell; overall: Metric }
}>()
defineEmits<{ clear: [] }>()

defineSlots<{ below?: () => unknown }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()
const duration = useStepDuration()

const points = (value: number) => new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1, signDisplay: 'exceptZero' }).format(value)

const steps = computed(() => funnelOf(props.state)?.steps ?? [])
const reached = computed(() => props.report.steps.map((step) => step.reached))

/** The same funnel over the previous period, for the change beside the conversion. */
const previous = useExplore(() => ({ ...setSplit(props.state, null), range: previousDays(props.resolved) }))
const change = computed(() => {
  const before = previous.query.data.value?.report
  const now = props.report.conversion
  if (before?.kind !== 'funnel' || before.conversion.t !== 'ratio' || now.t !== 'ratio') return null
  return Math.round((now.v - before.conversion.v) * 1000) / 10
})

/** Medians between steps; the last duration is step 1 to the last step, shown in the rail. */
const gaps = computed(() => props.report.durations.slice(0, -1).filter((d) => d.median.t === 'millis'))
const toConvert = computed(() => {
  const last = props.report.durations.at(-1)
  return last?.median.t === 'millis' ? last : null
})
const wide = useMediaQuery('(min-width: 768px)')
/** Room above the columns: below `md` the median sits under each label, so it needs a third line. */
const headroom = computed(() => (!wide.value && gaps.value.length > 0 ? 76 : 52))
const gapAt = (index: number) => gaps.value.find((d) => d.from_step === index - 1)

/** Overall conversion against the group's, in points. */
const againstAll = computed(() => {
  const own = props.report.conversion
  const all = props.group?.overall
  if (!all || own.t !== 'ratio' || all.t !== 'ratio') return null
  return { points: Math.round((own.v - all.v) * 1000) / 10, overall: format.metric(all) }
})

const box = useTemplateRef('box')
const { width, height } = useElementSize(box)
const drawn = computed(() =>
  ribbon(reached.value, props.report.leaks, { width: Math.max(0, width.value), height: Math.max(0, height.value), top: headroom.value })
)
const pitch = computed(() => (drawn.value.steps.length > 1 ? drawn.value.steps[1]!.x : drawn.value.width))
/** A label leaves room for the median tag beside it on wide screens. */
const labelWidth = computed(() => Math.max(48, wide.value && gaps.value.length > 0 ? pitch.value / 2 - 84 : pitch.value - 12))
const worst = computed(() => worstLeak(drawn.value.leaks))
const picked = ref<number | null>(null)
const focus = computed(() => drawn.value.leaks.find((leak) => leak.step === picked.value) ?? worst.value)

const caption = (group: Group | undefined): string => {
  const [first, ...rest] = conditions(group ?? null)
  if (!first) return ''
  const field = format.fieldLabel(first.field)
  const value = first.value?.t === 'string' ? first.value.v : null
  const text =
    first.field === 'event_name' && first.cmp === 'is_not_set'
      ? t('reports.funnel.stage.anyPageview')
      : first.field === 'event_name' && first.cmp === 'is_set'
        ? t('reports.funnel.stage.anyEvent')
        : first.cmp === 'is_set'
          ? t('reports.funnel.stage.isSet', { field })
          : first.cmp === 'is_not_set'
            ? t('reports.funnel.stage.notSet', { field })
            : (value ?? field)
  return rest.length > 0 ? `${text} +${rest.length}` : text
}

const kindLabels: Record<Exclude<LeakKind, 'page'>, MessageKey> = {
  exited: 'reports.funnel.stage.exited',
  back: 'reports.funnel.stage.back',
  other: 'reports.funnel.stage.other',
}
const where = (flow: Pick<RibbonFlow, 'kind' | 'path'>) => (flow.kind === 'page' ? (flow.path ?? '') : t(kindLabels[flow.kind]))

const link = (state: ExplorationState) => ({ name: pageOf(state), params: { siteId: state.site }, query: { s: encodeState(state) } })
const stepSessions = (index: number) => {
  const step = steps.value[index]
  return step ? link(sessionsFor(props.state, [step])) : null
}
const leakOf = (step: number) => props.report.leaks[step]
</script>

<template>
  <StageView :label="t('reports.funnel.title')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p
          v-if="group"
          class="mb-3 flex items-center gap-2 text-sm text-muted"
        >
          <span class="min-w-0 truncate">
            {{ format.fieldLabel(group.field) }}
            <b class="font-semibold text-primary">{{ format.cellLabel(group.field, group.cell) }}</b>
          </span>
          <button
            type="button"
            class="ml-auto shrink-0 cursor-pointer rounded-control text-accent-text underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            @click="$emit('clear')"
          >
            {{ t('reports.funnel.split.all') }}
          </button>
        </p>
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">{{ format.metric(report.conversion) }}</p>
        <p class="mt-3 text-[0.9375rem] text-muted tabular-nums">
          {{
            t('reports.funnel.conversionOf', {
              last: numbers.integer(reached.at(-1) ?? 0),
              first: numbers.integer(reached[0] ?? 0),
            })
          }}
        </p>
        <p
          v-if="againstAll"
          class="mt-1 text-sm text-muted tabular-nums"
        >
          {{
            againstAll.points === 0
              ? t('reports.funnel.split.same', { overall: againstAll.overall })
              : t(againstAll.points > 0 ? 'reports.funnel.split.above' : 'reports.funnel.split.below', {
                  points: Math.abs(againstAll.points),
                  overall: againstAll.overall,
                })
          }}
        </p>
        <p
          v-else-if="change !== null"
          class="mt-1 text-sm text-muted tabular-nums"
        >
          {{ change === 0 ? t('reports.funnel.stage.unchanged') : t('reports.funnel.stage.change', { change: points(change) }) }}
        </p>
        <p
          v-if="toConvert"
          class="mt-3 flex items-start gap-2 text-sm text-muted tabular-nums"
        >
          <Icon
            name="lucide:clock"
            size="14"
            class="mt-0.5 shrink-0"
            aria-hidden="true"
          />
          <span>
            {{
              t('reports.funnel.duration.toConvert', {
                median: duration.metric(toConvert.median) ?? '',
                from: duration.metric(toConvert.p25) ?? '',
                to: duration.metric(toConvert.p75) ?? '',
              })
            }}
          </span>
        </p>
      </div>

      <p class="border-b border-hairline py-5 text-[0.9375rem] text-primary">
        <template v-if="worst">
          <span
            class="mr-2 inline-block size-2 rounded-full bg-accent align-middle"
            aria-hidden="true"
          />
          {{
            t('reports.funnel.stage.worst', {
              step: worst.step + 1,
              caption: caption(steps[worst.step]),
              rate: numbers.percent(worst.rate),
            })
          }}
        </template>
        <template v-else>{{ t('reports.funnel.stage.noDrop') }}</template>
      </p>

      <section
        v-if="focus"
        class="min-h-0 flex-1 overflow-y-auto pt-5"
        :aria-labelledby="`leak-${focus.step}`"
      >
        <h2
          :id="`leak-${focus.step}`"
          class="text-sm font-semibold text-primary"
        >
          {{ t('reports.funnel.stage.after', { step: focus.step + 1 }) }}
        </h2>
        <p class="mt-1 text-sm text-muted tabular-nums">
          {{ t('reports.funnel.stage.stopped', { count: numbers.integer(focus.dropped), reached: numbers.integer(reached[focus.step] ?? 0) }) }}
        </p>
        <ul class="mt-3">
          <li
            v-for="flow in focus.flows"
            :key="`${flow.kind}-${flow.path}`"
            class="flex h-8 items-center gap-3 text-[0.9375rem]"
          >
            <span
              class="size-2 shrink-0 rounded-full"
              :class="flow.kind === 'exited' ? 'bg-accent' : 'bg-series-comparison'"
              aria-hidden="true"
            />
            <span class="min-w-0 flex-1 truncate text-primary">{{ where(flow) }}</span>
            <span class="shrink-0 text-muted tabular-nums">{{ numbers.integer(flow.identities) }}</span>
          </li>
        </ul>
        <h3 class="mt-5 text-sm font-semibold text-primary">{{ t('reports.funnel.stage.sessions') }}</h3>
        <ul
          v-if="(leakOf(focus.step)?.sessions.length ?? 0) > 0"
          class="mt-2 grid grid-cols-2 gap-x-4"
        >
          <li
            v-for="id in leakOf(focus.step)?.sessions ?? []"
            :key="id"
          >
            <RouterLink
              :to="{ name: 'session', params: { siteId: state.site, sessionId: id } }"
              class="flex h-8 items-center rounded-control text-sm text-primary tabular-nums underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              {{ t('reports.funnel.stage.session', { id }) }}
            </RouterLink>
          </li>
        </ul>
        <p
          v-else
          class="mt-2 text-sm text-muted"
        >
          {{ t('reports.funnel.stage.noSessions') }}
        </p>
        <RouterLink
          v-if="stepSessions(focus.step)"
          :to="stepSessions(focus.step)!"
          class="mt-3 inline-flex h-8 items-center rounded-control text-sm font-medium text-accent-text underline-offset-4 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          {{ t('reports.funnel.stage.allSessions', { step: focus.step + 1 }) }}
        </RouterLink>
      </section>
    </template>

    <div class="flex min-h-0 flex-col gap-6 lg:h-full lg:overflow-y-auto">
    <figure
      ref="box"
      class="relative m-0 h-[26rem] w-full shrink-0 lg:min-h-[26rem] lg:flex-1"
    >
      <figcaption class="sr-only">{{ t('reports.funnel.stage.figure', { steps: reached.length }) }}</figcaption>
      <svg
        v-if="width > 0"
        class="absolute inset-0 size-full overflow-visible"
        :viewBox="`0 0 ${drawn.width} ${drawn.height}`"
        aria-hidden="true"
      >
        <path
          v-for="(d, index) in drawn.bands"
          :key="`band-${index}`"
          :d="d"
          class="fill-series-comparison/35"
        />
        <g
          v-for="leak in drawn.leaks"
          :key="`leak-${leak.step}`"
          class="cursor-pointer"
          @click="picked = leak.step"
        >
          <path
            v-for="flow in leak.flows"
            :key="`${flow.kind}-${flow.path}`"
            :d="flow.d"
            class="motion-safe:transition-colors"
            :class="focus?.step === leak.step ? (flow.kind === 'exited' ? 'fill-accent/70' : 'fill-accent/30') : 'fill-series-comparison/20 hover:fill-series-comparison/35'"
          />
        </g>
        <rect
          v-for="(step, index) in drawn.steps"
          :key="`step-${index}`"
          :x="step.x"
          :y="step.y"
          :width="step.width"
          :height="step.height"
          rx="2"
          class="fill-primary"
        />
      </svg>

      <template v-if="width > 0">
        <RouterLink
          v-for="(step, index) in drawn.steps"
          :key="`label-${index}`"
          :to="stepSessions(index) ?? ''"
          class="absolute top-0 -mx-1 block rounded-control px-1 text-sm hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          :class="index > 0 && index === drawn.steps.length - 1 ? 'text-right' : ''"
          :style="{
            ...(index > 0 && index === drawn.steps.length - 1 ? { right: '0px' } : { left: `${step.x}px` }),
            maxWidth: `${labelWidth}px`,
          }"
          :aria-label="t('reports.funnel.stage.stepLink', { step: index + 1, caption: caption(steps[index]), count: numbers.integer(step.reached) })"
        >
          <span class="block truncate font-medium text-primary">
            {{ index + 1 }}<template v-if="pitch > 96">&ensp;{{ caption(steps[index]) }}</template>
          </span>
          <span class="block truncate text-muted tabular-nums">
            {{ numbers.integer(step.reached) }}<template v-if="pitch > 96 && index > 0"> · {{ numbers.percent(step.share) }}</template>
          </span>
        </RouterLink>
        <template
          v-for="leak in drawn.leaks"
          :key="`labels-${leak.step}`"
        >
          <button
            v-for="flow in leak.flows"
            :key="`${flow.kind}-${flow.path}`"
            type="button"
            class="absolute flex h-6 -translate-y-1/2 cursor-pointer items-center gap-1.5 rounded-control px-1 text-xs whitespace-nowrap hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            :class="focus?.step === leak.step ? 'text-primary' : 'text-muted'"
            :style="{ left: `${flow.labelX}px`, top: `${flow.labelY}px`, maxWidth: `${Math.max(56, drawn.width - flow.labelX)}px` }"
            :aria-pressed="focus?.step === leak.step"
            :aria-label="t('reports.funnel.stage.flowLabel', { count: numbers.integer(flow.identities), step: leak.step + 1, where: where(flow) })"
            @click="picked = leak.step"
          >
            <span
              class="truncate"
              :class="focus?.step === leak.step && flow.kind === 'exited' ? 'font-semibold text-accent-text' : ''"
            >
              {{ where(flow) }}
            </span>
            <span class="tabular-nums">{{ numbers.integer(flow.identities) }}</span>
          </button>
        </template>
        <div
          v-for="index in drawn.steps.length - 1"
          :key="`gap-${index}`"
          class="absolute"
          :class="wide ? '-translate-x-1/2' : ''"
          :style="
            wide
              ? { left: `${(drawn.steps[index - 1]!.x + drawn.steps[index - 1]!.width + drawn.steps[index]!.x) / 2}px`, top: '0px' }
              : index === drawn.steps.length - 1
                ? { right: '0px', top: '44px' }
                : { left: `${drawn.steps[index]!.x}px`, top: '44px' }
          "
        >
          <StepGap
            v-if="gapAt(index)"
            :duration="gapAt(index)!"
            :placement="wide ? 'gap' : 'label'"
          />
        </div>
      </template>
    </figure>
    <slot name="below" />
    </div>
  </StageView>
</template>
