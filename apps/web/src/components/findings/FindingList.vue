<script setup lang="ts">
import { useIntervalFn, useNow } from '@vueuse/core'

import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '~/components/ui/sheet'
import { Skeleton } from '~/components/ui/skeleton'
import { useSiteFindings } from '~/composables/useSiteFindings'
import type { Group, Site } from '~/generated'
import { FINDINGS_MAX } from '~/generated/limits'
import { useI18n } from '~/plugins/i18n'

import { listState } from './actions'
import FindingBanner from './FindingBanner.vue'
import FindingCard from './FindingCard.vue'
import { useFindingText } from './useFindingText'

/**
 * The site's noticed changes under the trend chart: the first three as rows of one panel, the
 * lead one marked by the accent, the rest in a sheet. Findings are always for all traffic, so a filtered overview
 * says so. Facts only, from the site's last summary: when it is more than 30 minutes behind, a
 * notice takes the place of the findings.
 */
const props = defineProps<{ site: Site; filtered?: boolean }>()
const emit = defineEmits<{ filter: [filter: Group] }>()

const SHOWN = 3

const { t } = useI18n()
const now = useNow({ scheduler: (update) => useIntervalFn(update, 60_000) })
const { query, findings, stale, computedAt } = useSiteFindings(() => props.site.id, FINDINGS_MAX)
const words = useFindingText(() => props.site)

const shown = computed(() => findings.value.slice(0, SHOWN))
const listing = computed(() => listState(computedAt.value, now.value.getTime()))
const computedTime = computed(() => (computedAt.value === null ? '' : words.clock(computedAt.value)))
const open = ref(false)

const filter = (group: Group) => {
  open.value = false
  emit('filter', group)
}
</script>

<template>
  <section
    v-if="!query.isError.value"
    :aria-label="t('findings.label')"
    class="mb-6"
  >
    <Skeleton
      v-if="query.isPending.value"
      class="h-15 w-full rounded-panel"
    />
    <p
      v-else-if="listing === 'pending'"
      class="flex min-h-15 items-center gap-3 rounded-panel border border-hairline bg-card px-4.5 text-muted"
    >
      <Icon
        name="lucide:sparkles"
        size="20"
        aria-hidden="true"
      />
      {{ t('findings.pending') }}
    </p>
    <p
      v-else-if="listing === 'behind'"
      class="flex min-h-15 items-center gap-3 rounded-panel border border-hairline bg-card px-4.5 text-muted"
    >
      <Icon
        name="lucide:history"
        size="20"
        aria-hidden="true"
      />
      {{ t('findings.behind', { time: computedTime }) }}
    </p>
    <template v-else>
      <Panel
        v-if="shown.length > 0"
        :title="t('findings.label')"
        :meta="stale ? `${computedTime} · ${t('findings.updating')}` : computedTime"
        flush
      >
        <template
          v-if="filtered || findings.length > SHOWN"
          #actions
        >
          <span
            v-if="filtered"
            class="text-[0.875rem] text-muted"
          >
            {{ t('findings.allTraffic') }}
          </span>
          <Button
            v-if="findings.length > SHOWN"
            variant="ghost"
            size="sm"
            class="text-muted"
            @click="open = true"
          >
            {{ t('findings.showAll', { count: findings.length }) }}
          </Button>
        </template>
        <div class="divide-y divide-divider border-t border-divider">
          <FindingBanner
            v-for="(finding, index) in shown"
            :key="`${finding.rule}:${finding.rank}`"
            :site="site"
            :finding="finding"
            :text="words.text(finding)"
            :lead="index === 0"
            @filter="filter"
          />
        </div>
      </Panel>
      <p
        v-else
        class="flex min-h-15 items-center gap-3 rounded-panel border border-hairline bg-card px-4.5 text-muted"
      >
        <Icon
          name="lucide:sparkles"
          size="20"
          aria-hidden="true"
        />
        {{ t('findings.empty') }}
      </p>
      <Sheet
        v-if="findings.length > SHOWN"
        v-model:open="open"
      >
        <SheetContent class="flex flex-col gap-3 overflow-y-auto">
          <SheetTitle>{{ t('findings.all.title') }}</SheetTitle>
          <SheetDescription>{{ t('findings.all.description') }}</SheetDescription>
          <FindingCard
            v-for="finding in findings"
            :key="`${finding.rule}:${finding.rank}`"
            :site="site"
            :finding="finding"
            :text="words.text(finding)"
            :time="words.clock(finding.computed_at)"
            @filter="filter"
          />
        </SheetContent>
      </Sheet>
    </template>
  </section>
</template>
