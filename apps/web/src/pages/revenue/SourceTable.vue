<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'

import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useFormat } from '~/composables/useFormat'
import type { DimensionField, VisitorLabel } from '~/generated'
import type { Source } from '~/lib/revenue-summary'
import { useI18n } from '~/plugins/i18n'

/**
 * Sources ranked by net revenue, each with its share of revenue (accent bar) over its share of
 * visitors (gray bar), net per visitor and conversion. A source paying 1.5 times its traffic
 * share or more takes the accent, two thirds or less the destructive tone; the bars carry the
 * same, so color is a second signal. Each named source opens Explore narrowed to it.
 */
const props = defineProps<{
  sources: Source[]
  field: DimensionField
  visitorLabel: VisitorLabel
  /** Formats minor units of the chosen currency. */
  amount: (units: number) => string
  to: (source: Source) => RouteLocationRaw | null
}>()

const { t } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()

/** Bars share one scale, the largest share of either kind, so they compare across rows. */
const widest = computed(() => Math.max(0.01, ...props.sources.flatMap((source) => [source.revenueShare, source.visitorShare])))
const width = (share: number) => `${(share / widest.value) * 100}%`
const name = (source: Source) => (source.key ? format.cellLabel(props.field, source.key) : t('revenue.summary.traffic.other'))
/** Conversion keeps one decimal: 2.7 % and 3.4 % are different stories. */
const rate = (ratio: number, { paying, visitors }: Source) => format.metric({ t: 'ratio', v: ratio, numerator: paying, denominator: visitors })
const tones = { more: 'text-accent-text', less: 'text-destructive', even: 'text-muted' } as const
const perVisitorLabel = computed(() =>
  props.visitorLabel === 'visitors' ? t('revenue.summary.traffic.perVisitor') : t('revenue.summary.traffic.perVisitorDay')
)
</script>

<template>
  <div class="-mx-4.5 overflow-x-auto px-4.5">
    <table class="w-full min-w-[36rem] text-sm tabular-nums">
      <thead>
        <tr class="text-[0.8125rem] text-muted">
          <th
            scope="col"
            class="pb-2.5 text-left font-medium"
          >
            {{ format.fieldLabel(field) }}
          </th>
          <th
            scope="col"
            class="w-[34%] px-3 pb-2.5 text-left font-medium whitespace-nowrap"
          >
            {{ t('revenue.summary.traffic.shares') }}
          </th>
          <th
            scope="col"
            class="px-3 pb-2.5 text-right font-medium"
          >
            {{ format.measureLabel('revenue_net') }}
          </th>
          <th
            scope="col"
            class="px-3 pb-2.5 text-right font-medium whitespace-nowrap"
          >
            {{ perVisitorLabel }}
          </th>
          <th
            scope="col"
            class="pb-2.5 text-right font-medium"
          >
            {{ t('revenue.summary.traffic.conversion') }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="source in sources"
          :key="source.key ? JSON.stringify(source.key) : 'other'"
          class="h-11 border-t border-divider"
        >
          <th
            scope="row"
            class="max-w-48 truncate text-left font-normal"
          >
            <RouterLink
              v-if="to(source)"
              :to="to(source)!"
              class="rounded-control text-primary hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              {{ name(source) }}
            </RouterLink>
            <span
              v-else
              class="text-muted"
            >
              {{ name(source) }}
            </span>
          </th>
          <td class="px-3">
            <div class="flex items-center gap-2.5">
              <div class="grid min-w-0 flex-1 gap-[3px]">
                <span
                  class="block h-1.5 rounded-xs bg-accent"
                  :style="{ width: width(source.revenueShare) }"
                />
                <span
                  class="block h-1.5 rounded-xs bg-fill-strong"
                  :style="{ width: width(source.visitorShare) }"
                />
              </div>
              <span
                class="w-24 shrink-0 text-[0.8125rem] whitespace-nowrap"
                :class="tones[source.pays]"
              >
                {{ t('revenue.summary.traffic.shareOf', { revenue: numbers.percent(source.revenueShare), visitors: numbers.percent(source.visitorShare) }) }}
              </span>
            </div>
          </td>
          <td class="px-3 text-right whitespace-nowrap">{{ amount(source.net) }}</td>
          <td class="px-3 text-right whitespace-nowrap">{{ source.perVisitor === null ? '–' : amount(source.perVisitor) }}</td>
          <td class="text-right whitespace-nowrap">{{ source.conversion === null ? '–' : rate(source.conversion, source) }}</td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
