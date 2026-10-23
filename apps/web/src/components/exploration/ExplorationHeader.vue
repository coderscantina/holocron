<script setup lang="ts">
import { createReusableTemplate } from '@vueuse/core'

import { ApiError } from '~/api'
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useErrorMessage } from '~/composables/useErrorMessage'
import type { StateProblem } from '~/composables/useExploration'
import type { Notice } from '~/composables/useExplorationScreen'
import { useFilterFields } from '~/composables/useFilterFields'
import type { ExplorationState, ExploreResult, Site } from '~/generated'
import { useI18n, type MessageKey } from '~/plugins/i18n'

import ExplorationControls from './ExplorationControls.vue'
import ExportMenu from './ExportMenu.vue'
import FreshnessNote from './FreshnessNote.vue'
import PinDialog from './PinDialog.vue'
import SavedViews from './SavedViews.vue'
import TitleFreshness from './TitleFreshness.vue'

/**
 * Everything above an exploration's results: the scope chips in the top bar (range,
 * comparison, interval, filters and segments), saved views, export, pinning and refresh, the
 * result's freshness, action notices, and the states that stop a calculation (an unreadable
 * link, a refused state). While a new result loads, the previous one stays on screen without a
 * visible note: the spinning refresh icon shows the load, a status line tells screen readers. With `heading` it renders the page title row with those actions, and
 * the `context` slot puts a page's own view switch beside the title; without `heading` the page
 * brings its own title row and the actions sit in a row of their own.
 */
const props = defineProps<{
  site: Site
  state: ExplorationState | null
  problem: StateProblem | null
  oversize: boolean
  result: ExploreResult | null
  fetching: boolean
  previous: boolean
  refusal: ApiError | null
  refused: Record<string, string>
  notice: Notice | null
  /** Default card title when pinning. */
  title: string
  heading?: { title: string; description?: string }
}>()

defineSlots<{
  /** Beside the title in the title row, for a view switch that configures the whole page. */
  context?: () => unknown
}>()

const emit = defineEmits<{
  commit: [state: ExplorationState]
  reset: []
  refresh: []
  notice: [notice: Notice]
}>()

const { t } = useI18n()
const { message } = useErrorMessage()
const { fields } = useFilterFields(() => props.site.id)

const problems: Record<StateProblem, MessageKey> = {
  unreadable: 'explore.states.unreadable',
  version: 'explore.states.version',
  oversize: 'explore.states.oversize',
  site: 'explore.states.site',
}

/** Whether the typical-weekday baseline of every measure lacks history (0015: fewer than 4 days). */
const insufficient = computed(
  () =>
    props.state?.compare === 'typical_weekday' &&
    (props.result?.main.compare ?? []).length > 0 &&
    (props.result?.main.compare ?? []).every((comparison) => comparison.reason === 'insufficient_history')
)

const [DefineActions, ActionsRow] = createReusableTemplate()

const can = (capability: Site['capabilities'][number]) => props.site.capabilities.includes(capability)
const onNotice = (text: string, tone: Notice['tone']) => emit('notice', { message: text, tone })
</script>

<template>
  <DefineActions>
    <div class="flex flex-wrap items-center gap-0.5">
      <SavedViews
        v-if="state"
        :state="state"
        @apply="(next) => emit('commit', next)"
        @notice="onNotice"
      />
      <ExportMenu
        v-if="state && can('site.export')"
        :state="state"
        @notice="onNotice"
      />
      <PinDialog
        v-if="state"
        :state="state"
        :workspace="site.workspace"
        :title="title"
        @notice="onNotice"
      />
      <SimpleTooltip
        :tooltip="t('explore.refresh')"
        as-child
      >
        <Button
          variant="ghost"
          size="icon"
          class="size-7 text-muted hover:text-primary"
          :aria-label="t('explore.refresh')"
          :disabled="fetching"
          @click="emit('refresh')"
        >
          <Icon
            name="lucide:refresh-cw"
            :class="fetching ? 'animate-spin motion-reduce:animate-none' : ''"
          />
        </Button>
      </SimpleTooltip>
    </div>
  </DefineActions>
  <PageHeader
    v-if="heading"
    :title="heading.title"
    :description="heading.description"
  >
    <template
      v-if="$slots.context"
      #eyebrow
    >
      <slot name="context" />
    </template>
    <template
      v-if="state"
      #actions
    >
      <ActionsRow />
    </template>
    <template #freshness>
      <TitleFreshness :result="result" />
    </template>
  </PageHeader>
  <EmptyState
    v-if="problem"
    icon="lucide:link-2-off"
    tone="error"
    :title="t(problems[problem])"
    :description="t('explore.states.problemHint')"
    class="mx-auto max-w-lg"
  >
    <Button
      variant="outline"
      @click="emit('reset')"
    >
      {{ t('explore.reset') }}
    </Button>
  </EmptyState>
  <div
    v-else-if="state"
    class="mb-4 grid gap-3 [&:not(:has(>:not(:empty)))]:hidden"
  >
    <ExplorationControls
      :state="state"
      :resolved="previous ? null : (result?.resolved ?? null)"
      :insufficient="insufficient"
      :fields="fields"
      :refused="refused"
      :can-save-segments="can('site.definitions.write')"
      :retention="site.retention"
      @commit="(next) => emit('commit', next)"
      @notice="onNotice"
    />
    <div
      v-if="!heading"
      class="flex justify-end"
    >
      <ActionsRow />
    </div>
    <FreshnessNote
      v-if="result"
      :result="result"
    />
    <Alert
      v-if="oversize"
      color="warning"
      role="alert"
    >
      <p class="text-sm">{{ t('explore.states.oversizeEdit') }}</p>
    </Alert>
    <Alert
      v-if="notice"
      :color="notice.tone === 'error' ? 'destructive' : 'success'"
      :role="notice.tone === 'error' ? 'alert' : 'status'"
    >
      <p class="text-sm">{{ notice.message }}</p>
    </Alert>
    <Alert
      v-if="refusal"
      color="destructive"
      role="alert"
    >
      <p class="text-sm">
        <strong class="font-semibold">{{ t('explore.states.refused') }}.</strong>
        {{ message(refusal) }}
      </p>
    </Alert>
  </div>
  <p
    class="sr-only"
    role="status"
  >
    {{ previous ? t('explore.states.previous') : '' }}
  </p>
</template>
