<script setup lang="ts">
import { Translation } from 'vue-i18n'

import FlowDiagram from '~/components/flow/FlowDiagram.vue'
import FlowFocus from '~/components/flow/FlowFocus.vue'
import type { FlowDiagramLink, FlowDiagramNode } from '~/components/flow/layout'
import StageView from '~/components/stage/StageView.vue'
import { useFormat } from '~/composables/useFormat'
import type { ExplorationState, JourneyResult, JourneySequence } from '~/generated'
import { biggestDropOff, emphasis, journeyFlow, type JourneyNode } from '~/lib/journey-flow'
import { useI18n } from '~/plugins/i18n'
import { eventsFor, fieldEquals, journeyOf, sessionsFor } from '~/services/reports'

/**
 * Journeys as a flow: from the first step on the left through every step to where sessions
 * end, the most common path in the accent. Choosing a node, a path or the biggest drop-off
 * focuses the flow on the sessions through it; the column beside says how many that are and
 * opens their sessions or events.
 */
const props = defineProps<{ state: ExplorationState; report: JourneyResult }>()

const { t } = useI18n()
const format = useFormat()

const PER_COLUMN = 6
const TOP_PATHS = 5

type Focus = { kind: 'node'; id: string } | { kind: 'page'; value: string } | { kind: 'path'; sequence: JourneySequence }
const focus = shallowRef<Focus | null>(null)
watch(
  () => props.report,
  () => (focus.value = null)
)

const journey = computed(() => journeyOf(props.state))
const length = computed(() => journey.value?.length ?? 0)
const field = computed(() => journey.value?.value ?? 'path')
const people = computed(() => journey.value?.scope === 'person')
const flow = computed(() => journeyFlow(props.report.sequences, length.value, PER_COLUMN))
const total = computed(() => props.report.entry_sessions)
const shown = computed(() => props.report.sequences.reduce((sum, sequence) => sum + sequence.sessions, 0))
const top = computed(() => props.report.sequences[0] ?? null)

const passes = (sequence: JourneySequence, route: string[], current: Focus) => {
  switch (current.kind) {
    case 'node':
      return route.includes(current.id)
    case 'page':
      return sequence.steps.includes(current.value)
    case 'path':
      return sequence === current.sequence
  }
}
const emphasised = computed(() => {
  const current = focus.value
  return emphasis(flow.value, props.report.sequences, (sequence, route) =>
    current ? passes(sequence, route, current) : sequence === top.value
  )
})

const nodeLabel = (node: JourneyNode) =>
  node.kind === 'step' ? node.value : node.kind === 'other' ? t('reports.journey.flow.other') : t('reports.journey.flow.exit')
const nodes = computed(() =>
  flow.value.nodes.map(
    (node): FlowDiagramNode => ({
      id: node.id,
      column: node.column,
      label: nodeLabel(node),
      figure: format.compact(node.sessions),
      tone: node.kind === 'step' ? undefined : 'quiet',
    })
  )
)
const links = computed(() =>
  flow.value.links.map((link): FlowDiagramLink => ({ ...link, emphasis: emphasised.value.get(`${link.source}>${link.target}`) ?? 0 }))
)
const selected = computed(() => {
  const current = focus.value
  if (!current) return null
  return current.kind === 'node' ? current.id : current.kind === 'page' ? `page:${current.value}` : 'path'
})
const select = (id: string | null) => (focus.value = id ? { kind: 'node', id } : null)

const share = (sessions: number) => (total.value > 0 ? sessions / total.value : 0)
const dropOff = computed(() => biggestDropOff(props.report.sequences, length.value))

const focused = computed(() => {
  const current = focus.value
  if (!current) return null
  const node = current.kind === 'node' ? flow.value.nodes.find((candidate) => candidate.id === current.id) : undefined
  const value = current.kind === 'page' ? current.value : node?.kind === 'step' ? node.value : null
  const sessions = props.report.sequences
    .filter((sequence) => passes(sequence, flow.value.route(sequence), current))
    .reduce((sum, sequence) => sum + sequence.sessions, 0)
  const title =
    current.kind === 'path' ? chain(current.sequence) : current.kind === 'page' ? current.value : node ? nodeLabel(node) : ''
  return { title, value, sessions }
})

const paths = computed(() => {
  const current = focus.value
  return props.report.sequences
    .filter((sequence) => !current || current.kind === 'path' || passes(sequence, flow.value.route(sequence), current))
    .slice(0, TOP_PATHS)
})
const ends = (sequence: JourneySequence) => sequence.steps.length < length.value
const chain = (sequence: JourneySequence) =>
  [...sequence.steps, ...(ends(sequence) ? [t('reports.journey.flow.exit')] : [])].join(' → ')

const drills = computed(() => {
  const value = focused.value?.value
  if (value === undefined || value === null) return null
  const condition = [fieldEquals(props.state, field.value, value)]
  return { sessions: sessionsFor(props.state, condition), events: eventsFor(props.state, condition) }
})
</script>

<template>
  <StageView :label="t('reports.journey.flow.rail')">
    <template #rail>
      <div class="border-b border-hairline pb-6">
        <p class="text-[3.25rem] leading-none font-bold tracking-[-0.025em] text-primary tabular-nums">
          {{ top ? format.percent(share(top.sessions)) : '–' }}
        </p>
        <p class="mt-3 text-[0.9375rem] text-muted">
          {{ people ? t('reports.journey.flow.topShare.person') : t('reports.journey.flow.topShare.session') }}
        </p>
        <button
          v-if="top"
          type="button"
          class="mt-2 max-w-full truncate rounded-sm text-left text-[0.9375rem] font-medium text-accent-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
          :aria-pressed="focus?.kind === 'path' && focus.sequence === top"
          @click="focus = focus?.kind === 'path' && focus.sequence === top ? null : { kind: 'path', sequence: top }"
        >
          {{ chain(top) }}
        </button>
        <p
          v-if="dropOff && dropOff.reached > 0"
          class="mt-5 text-[0.9375rem] leading-relaxed text-primary"
        >
          <Translation
            :keypath="people ? 'reports.journey.flow.dropOff.person' : 'reports.journey.flow.dropOff.session'"
            scope="global"
            tag="span"
          >
            <template #share>
              <span class="font-semibold tabular-nums">{{ format.percent(dropOff.exits / dropOff.reached) }}</span>
            </template>
            <template #page>
              <button
                type="button"
                class="rounded-sm font-semibold text-primary underline decoration-hairline underline-offset-4 hover:decoration-current focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                :aria-pressed="focus?.kind === 'page' && focus.value === dropOff.value"
                @click="focus = focus?.kind === 'page' && focus.value === dropOff.value ? null : { kind: 'page', value: dropOff.value }"
              >
                {{ dropOff.value }}
              </button>
            </template>
          </Translation>
        </p>
      </div>

      <FlowFocus
        v-if="focused"
        :title="focused.title"
        :summary="
          people
            ? t('reports.journey.flow.through.person', { count: format.integer(focused.sessions), share: format.percent(share(focused.sessions)) })
            : t('reports.journey.flow.through.session', { count: format.integer(focused.sessions), share: format.percent(share(focused.sessions)) })
        "
        :sessions="drills?.sessions"
        :events="drills?.events"
        @clear="focus = null"
      />

      <h2 class="pt-5 pb-1 text-sm font-medium text-muted">
        {{ focus && focus.kind !== 'path' ? t('reports.journey.flow.pathsThrough') : t('reports.journey.flow.paths') }}
      </h2>
      <ol class="min-h-0 flex-1 overflow-y-auto">
        <li
          v-for="(sequence, index) in paths"
          :key="sequence.steps.join('\u0000')"
        >
          <button
            type="button"
            class="flex h-10 w-full items-center gap-3 rounded-sm text-left text-[0.9375rem] hover:bg-fill focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring"
            :aria-pressed="focus?.kind === 'path' && focus.sequence === sequence"
            @click="focus = focus?.kind === 'path' && focus.sequence === sequence ? null : { kind: 'path', sequence }"
          >
            <span
              class="size-2 shrink-0 rounded-full"
              :class="(focus?.kind === 'path' ? focus.sequence === sequence : index === 0 && !focus) ? 'bg-accent' : 'bg-series-comparison'"
              aria-hidden="true"
            />
            <span class="min-w-0 flex-1 truncate text-primary">{{ chain(sequence) }}</span>
            <span class="shrink-0 text-muted tabular-nums">{{ format.percent(share(sequence.sessions)) }}</span>
          </button>
        </li>
      </ol>
      <p class="pt-3 text-xs text-muted">
        {{
          t('reports.journey.flow.coverage', {
            shown: format.integer(report.sequences.length),
            distinct: format.integer(report.distinct_sequences),
            share: format.percent(share(shown)),
          })
        }}
      </p>
    </template>

    <div class="flex h-[36rem] flex-col lg:h-full">
      <div class="mb-4 shrink-0">
        <slot name="toolbar" />
      </div>
      <p class="mb-3 flex shrink-0 justify-between gap-4 pr-12 text-xs text-muted">
        <span>{{ t('reports.journey.flow.first') }}</span>
        <span>{{ t('reports.journey.flow.hint') }}</span>
      </p>
      <div class="min-h-0 flex-1 pb-12">
        <FlowDiagram
          :nodes="nodes"
          :links="links"
          :label="t('reports.journey.flow.label')"
          :selected="selected"
          @select="select"
        />
      </div>
    </div>
  </StageView>
</template>
