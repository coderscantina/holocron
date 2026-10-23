<script setup lang="ts">
import ExportMenu from '~/components/exploration/ExportMenu.vue'
import FreshnessNote from '~/components/exploration/FreshnessNote.vue'
import MeasureCards from '~/components/exploration/MeasureCards.vue'
import RangeMenu from '~/components/exploration/RangeMenu.vue'
import ResultTable from '~/components/exploration/ResultTable.vue'
import TrendChart from '~/components/exploration/TrendChart.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Card } from '~/components/ui/card'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useSharedExploration, type Share } from '~/composables/useSharedView'
import type { Range, SharedView } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import RecordingsLink from './RecordingsLink.vue'
import ShareHeader from './ShareHeader.vue'

/**
 * A shared exploration. The viewer may page, sort, drill down and pick a range inside the
 * share's limits; anything outside answers `share_scope_exceeded` and offers the shared view.
 */
const props = defineProps<{ share: Share; view: SharedView }>()

const { t } = useI18n()
const { message } = useErrorMessage()
const format = useExplorationFormat()
const exploration = useSharedExploration(props.share)

const state = computed(() => exploration.state.value)
const result = computed(() => exploration.result.data.value ?? null)

const labels = computed(() =>
  (state.value?.measures ?? []).map((measure) =>
    (measure === 'visitor_days' || measure === 'visitors') && result.value
      ? format.measureLabel(result.value.visitor_label)
      : format.measureLabel(measure)
  )
)
const series = computed(() => state.value?.visualization === 'line' || state.value?.visualization === 'bar')
const splitFields = computed(() => exploration.shared.value?.limits.dimensions ?? [])

const pickRange = (range: Range) => {
  const current = state.value
  if (current) exploration.commit({ ...current, range, page: { ...current.page, index: 0 } })
}
</script>

<template>
  <template v-if="state">
    <ShareHeader :view="view">
      <RangeMenu
        :range="state.range"
        @change="pickRange"
      />
      <Button
        variant="outline"
        size="sm"
        :loading="exploration.result.isFetching.value"
        @click="exploration.refresh()"
      >
        {{ t('explore.refresh') }}
      </Button>
      <RecordingsLink v-if="view.recordings" />
      <ExportMenu
        v-if="view.export"
        :state="state"
        :client="share.scope.client"
      />
    </ShareHeader>

    <Alert
      v-if="exploration.exceeded.value"
      color="warning"
      class="mb-4"
    >
      <p class="text-sm">{{ t('errors.share_scope_exceeded') }}</p>
      <Button
        variant="outline"
        size="sm"
        class="mt-2"
        @click="exploration.reset()"
      >
        {{ t('share.reset') }}
      </Button>
    </Alert>
    <Alert
      v-else-if="exploration.result.isError.value"
      color="destructive"
      class="mb-4"
    >
      <p class="text-sm">{{ message(exploration.result.error.value) }}</p>
    </Alert>

    <FreshnessNote
      v-if="result"
      :result="result"
      full
      class="mb-4"
    />
    <MeasureCards
      class="mb-4"
      :measures="state.measures"
      :cut="result?.main ?? null"
      :visitor-label="result?.visitor_label ?? null"
    />
    <Card
      v-if="series"
      class="mb-4 p-5 max-sm:px-4"
    >
      <TrendChart
        v-if="result?.main.series"
        :series="result.main.series"
        :measures="state.measures.slice(0, 2)"
        :labels="labels.slice(0, 2)"
        :visualization="state.visualization === 'line' ? 'line' : 'bar'"
        :annotations="[]"
      />
      <Skeleton
        v-else
        class="h-56 w-full"
      />
    </Card>
    <template v-if="state.dimensions.length > 0">
      <ResultTable
        v-if="result"
        :state="state"
        :cut="result.main"
        :visitor-label="result.visitor_label"
        :split-fields="splitFields"
        @commit="exploration.commit"
      />
      <Skeleton
        v-else
        class="h-64 w-full"
      />
    </template>
  </template>
</template>
