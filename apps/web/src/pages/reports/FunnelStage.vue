<script setup lang="ts">
import { useElementSize } from '@vueuse/core'

import StageView from '~/components/stage/StageView.vue'
import { useExplore } from '~/composables/useExplore'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { ExplorationState, FunnelResult, Group, ResolvedRange } from '~/generated'
import { previousDays, ribbon, worstLeak, type LeakKind, type RibbonFlow } from '~/lib/funnel-ribbon'
import { useI18n, type MessageKey } from '~/plugins/i18n'
import { conditions, encodeState } from '~/services/exploration'
import { funnelOf, sessionsFor } from '~/services/reports'
import { pageOf } from '~/lib/state-pages'

/**
 * The funnel as a ribbon that narrows at each step, with the visitors who stop peeling off
 * below it by where they went next. The rail holds the conversion with its change against the
 * previous period, the worst step in one sentence, and the leak in focus: where its visitors
 * went and their latest sessions. The worst leak is in focus until another is picked.
 */
const props = defineProps<{ state: ExplorationState; report: FunnelResult; resolved: ResolvedRange }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()

const points = (value: number) => new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1, signDisplay: 'exceptZero' }).format(value)

const steps = computed(() => funnelOf(props.state)?.steps ?? [])
const reached = computed(() => props.report.steps.map((step) => step.reached))

/** The same funnel over the previous period, for the change beside the conversion. */
const previous = useExplore(() => ({ ...props.state, range: previousDays(props.resolved) }))
const change = computed(() => {
  const before = previous.query.data.value?.report
  const now = props.report.conversion
  if (before?.kind !== 'funnel' || before.conversion.t !== 'ratio' || now.t !== 'ratio') return null
  return Math.round((now.v - before.conversion.v) * 1000) / 10
})

const box = useTemplateRef('box')
const { width, height } = useElementSize(box)
const drawn = computed(() =>
  ribbon(reached.value, props.report.leaks, { width: Math.max(0, width.value), height: Math.max(0, height.value) })
)
const pitch = computed(() => (drawn.value.steps.length > 1 ? drawn.value.steps[1]!.x : drawn.value.width))
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
          v-if="change !== null"
          class="mt-1 text-sm text-muted tabular-nums"
        >
          {{ change === 0 ? t('reports.funnel.stage.unchanged') : t('reports.funnel.stage.change', { change: points(change) }) }}
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

    <figure
      ref="box"
      class="relative m-0 h-[26rem] w-full lg:h-full"
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
            maxWidth: `${Math.max(48, pitch - 12)}px`,
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
      </template>
    </figure>
  </StageView>
</template>
