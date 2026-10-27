<script setup lang="ts">
import ExplorationHeader from '~/components/exploration/ExplorationHeader.vue'
import { Alert } from '~/components/ui/alert'
import { Skeleton } from '~/components/ui/skeleton'
import type { useExplorationScreen } from '~/composables/useExplorationScreen'
import type { ExplorationState, ExploreResult, ReportResult, Site } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { complete } from '~/services/reports'

/**
 * The frame of a report screen: the title row with the site's domain and the shared exploration
 * actions (saved views, export, pinning, refresh, freshness), the report's builder, then its
 * result. A builder
 * with an empty step or goal says so instead of sending a state the server would refuse.
 */
const props = defineProps<{
  site: Site
  screen: ReturnType<typeof useExplorationScreen>
  title: string
  /** What Pin to board stores, when the screen shows part of its state's result. */
  pinned?: ExplorationState | null
}>()

defineSlots<{
  /** A view switch beside the title. */
  context?: () => unknown
  builder?: (props: { state: ExplorationState }) => unknown
  result: (props: { state: ExplorationState; result: ExploreResult; report: ReportResult }) => unknown
}>()

const { t } = useI18n()
const state = computed(() => props.screen.state.value)
const result = computed(() => props.screen.result.value)
const ready = computed(() => (state.value ? complete(state.value) : false))
/** A result of another kind is a placeholder from the previous screen and is never drawn. */
const report = computed(() => {
  const value = result.value?.report
  return value && value.kind === state.value?.report?.kind ? value : null
})
</script>

<template>
  <ExplorationHeader
    :site="site"
    :state="state"
    :problem="screen.problem.value"
    :oversize="screen.oversize.value"
    :result="result"
    :fetching="screen.query.isFetching.value"
    :previous="screen.previous.value"
    :refusal="screen.refusal.value"
    :refused="screen.refused.value"
    :notice="screen.notice.value"
    :title="title"
    :pinned="pinned"
    :heading="{ title, description: site.domain }"
    @commit="screen.commit"
    @reset="screen.reset"
    @refresh="screen.refresh"
    @notice="(notice) => (screen.notice.value = notice)"
  >
    <template
      v-if="$slots.context"
      #context
    >
      <slot name="context" />
    </template>
  </ExplorationHeader>

  <template v-if="state">
    <div
      v-if="$slots.builder"
      class="mb-4"
    >
      <slot
        name="builder"
        :state="state"
      />
    </div>
    <Alert
      v-if="!ready"
      color="warning"
      role="status"
      class="mb-4"
    >
      <p class="text-sm">{{ t('reports.incomplete') }}</p>
    </Alert>
    <template v-else-if="!screen.refusal.value">
      <slot
        v-if="result && report"
        name="result"
        :state="state"
        :result="result"
        :report="report"
      />
      <Skeleton
        v-else
        class="h-64 w-full"
      />
    </template>
  </template>
</template>
