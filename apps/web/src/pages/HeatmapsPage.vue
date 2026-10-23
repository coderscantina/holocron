<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import PageHeader from '~/components/PageHeader.vue'
import SiteBoundary from '~/components/SiteBoundary.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { Skeleton } from '~/components/ui/skeleton'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '~/components/ui/tabs'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import { usePageMeta } from '~/composables/usePageMeta'
import {
  heatmapRanges,
  isHeatmapRange,
  lastDays,
  useHeatmapCells,
  useHeatmapPages,
  type HeatmapRange,
} from '~/composables/useRecordings'
import type { ViewportClass } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import FilterMenu from './recordings/FilterMenu.vue'
import HeatmapCanvas from './recordings/HeatmapCanvas.vue'

const viewports = ['desktop', 'tablet', 'mobile'] as const satisfies readonly ViewportClass[]
/** The width each viewport class is drawn at. */
const referenceWidth: Record<ViewportClass, number> = { desktop: 1440, tablet: 1024, mobile: 390 }
const TOP_CELLS = 10

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const format = useFormat()
const { message } = useErrorMessage()

usePageMeta(() => t('nav.heatmaps'))

const siteId = computed(() => String(route.params.siteId))
const queryText = (name: string) => (typeof route.query[name] === 'string' ? route.query[name] : '')
const setQuery = (patch: Record<string, string | undefined>) => router.replace({ query: { ...route.query, ...patch } })

const range = computed<HeatmapRange>({
  get: () => {
    const value = queryText('range')
    return isHeatmapRange(value) ? value : '30d'
  },
  set: (value) => setQuery({ range: value }),
})
const viewport = computed<ViewportClass>({
  get: () => viewports.find((entry) => entry === queryText('viewport')) ?? 'desktop',
  set: (value) => setQuery({ viewport: value }),
})
const mode = computed({
  get: () => (queryText('mode') === 'scroll' ? 'scroll' : 'clicks'),
  set: (value: string | number) => setQuery({ mode: value === 'scroll' ? 'scroll' : undefined }),
})

const period = computed(() => lastDays(heatmapRanges[range.value]))
const pages = useHeatmapPages(() => ({ site: siteId.value, ...period.value }))
const pageList = computed(() => pages.data.value?.pages ?? [])
const selected = computed(() => queryText('page') || pageList.value[0]?.path || null)
const cells = useHeatmapCells(() =>
  selected.value ? { site: siteId.value, path: selected.value, viewport: viewport.value, ...period.value } : null
)
const topCells = computed(() => cells.data.value?.clicks.slice(0, TOP_CELLS) ?? [])
const share = (part: number, whole: number) => (whole > 0 ? part / whole : 0)
const percent = (part: number, whole: number) => format.percent(share(part, whole))

const rangeOptions = computed(() =>
  Object.keys(heatmapRanges)
    .filter(isHeatmapRange)
    .map((value) => ({ value, label: t(`heatmaps.ranges.${value}`) }))
)
const pagesId = useId()
const viewportOptions = computed(() => viewports.map((value) => ({ value, label: t(`heatmaps.viewports.${value}`) })))
</script>

<template>
  <SiteBoundary
    v-slot="{ site }"
    :site-id="siteId"
  >
    <PageHeader
      :title="t('nav.heatmaps')"
      :description="site.domain"
    >
      <template #actions>
        <FilterMenu
          v-model="range"
          :label="t('heatmaps.range')"
          :options="rangeOptions"
        />
        <FilterMenu
          v-model="viewport"
          :label="t('heatmaps.viewport')"
          :options="viewportOptions"
        />
        <Button
          v-if="site.capabilities.includes('site.configure')"
          as-child
          variant="fill"
          class="text-sm"
        >
          <RouterLink :to="{ name: 'recording-settings', params: { siteId } }">
            <Icon
              name="lucide:settings-2"
              aria-hidden="true"
            />
            {{ t('recording.title') }}
          </RouterLink>
        </Button>
      </template>
    </PageHeader>

    <div
      v-if="pages.isPending.value"
      class="grid gap-4 lg:grid-cols-[20rem_minmax(0,1fr)]"
      :aria-label="t('states.loading')"
      aria-busy="true"
    >
      <Skeleton class="h-64 w-full" />
      <Skeleton class="h-96 w-full" />
    </div>
    <EmptyState
      v-else-if="pages.isError.value"
      icon="lucide:triangle-alert"
      tone="error"
      :title="message(pages.error.value)"
    >
      <Button
        variant="outline"
        @click="pages.refetch()"
      >
        {{ t('actions.retry') }}
      </Button>
    </EmptyState>
    <EmptyState
      v-else-if="pageList.length === 0"
      icon="lucide:mouse-pointer-click"
      :title="t('heatmaps.empty.title')"
      :description="t('heatmaps.empty.description')"
    >
      <Button
        v-if="site.capabilities.includes('site.configure')"
        as-child
        variant="outline"
      >
        <RouterLink :to="{ name: 'recording-settings', params: { siteId } }">{{ t('recording.open') }}</RouterLink>
      </Button>
    </EmptyState>
    <div
      v-else
      class="grid items-start gap-4 lg:grid-cols-[20rem_minmax(0,1fr)]"
    >
      <Panel
        :id="pagesId"
        :title="t('heatmaps.pages')"
        flush
      >
        <div
          class="grid h-table-head grid-cols-[minmax(0,1fr)_3.5rem_3.5rem] items-center gap-3 bg-fill px-4 text-[0.875rem] font-medium text-muted"
          aria-hidden="true"
        >
          <span>{{ t('heatmaps.page') }}</span>
          <span class="text-right">{{ t('heatmaps.clicks') }}</span>
          <span class="text-right">{{ t('heatmaps.visits') }}</span>
        </div>
        <ul
          class="max-h-[30rem] overflow-y-auto"
          :aria-labelledby="pagesId"
        >
          <li
            v-for="page in pageList"
            :key="page.path"
            class="border-t border-divider first:border-t-0"
          >
            <button
              type="button"
              class="grid h-table-row w-full cursor-pointer grid-cols-[minmax(0,1fr)_3.5rem_3.5rem] items-center gap-3 px-4 text-left text-sm transition-colors hover:bg-fill/50 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring aria-pressed:bg-fill aria-pressed:shadow-[inset_2px_0_0_var(--color-accent)]"
              :aria-pressed="page.path === selected"
              :aria-label="`${page.path}, ${t('heatmaps.pageCounts', { clicks: format.integer(page.clicks), visits: format.integer(page.scroll_visits) })}`"
              @click="setQuery({ page: page.path })"
            >
              <span class="truncate font-medium text-primary">{{ page.path }}</span>
              <span class="text-right text-foreground tabular-nums">{{ format.integer(page.clicks) }}</span>
              <span class="text-right text-foreground tabular-nums">{{ format.integer(page.scroll_visits) }}</span>
            </button>
          </li>
        </ul>
      </Panel>

      <Tabs
        v-if="selected"
        v-model="mode"
        class="min-w-0"
      >
        <Panel
          :title="selected"
          :meta="
            mode === 'scroll'
              ? t('heatmaps.scrollTotal', { count: format.integer(cells.data.value?.scroll_visits ?? 0) })
              : t('heatmaps.total', { count: format.integer(cells.data.value?.total_clicks ?? 0) })
          "
          flush
        >
          <template #actions>
            <TabsList>
              <TabsTrigger value="clicks">{{ t('heatmaps.clicks') }}</TabsTrigger>
              <TabsTrigger value="scroll">{{ t('heatmaps.scroll') }}</TabsTrigger>
            </TabsList>
            <Button
              as-child
              variant="ghost"
              size="sm"
            >
              <RouterLink :to="{ name: 'replays', params: { siteId }, query: { path: selected, range: '14d' } }">
                <Icon
                  name="lucide:circle-play"
                  aria-hidden="true"
                />
                {{ t('heatmaps.replays') }}
              </RouterLink>
            </Button>
          </template>
          <TabsContent
            value="clicks"
            class="mt-0"
          >
            <p
              v-if="cells.data.value && cells.data.value.clicks.length === 0"
              class="px-4.5 pb-4.5 text-sm text-muted"
            >
              {{ t('heatmaps.noClicks') }}
            </p>
            <template v-else-if="cells.data.value">
              <div class="px-4.5 pb-4.5">
                <div class="mx-auto w-full max-w-3xl">
                  <HeatmapCanvas
                    :cells="cells.data.value.clicks"
                    :columns="cells.data.value.columns"
                    :row-height="cells.data.value.row_height"
                    :width="referenceWidth[viewport]"
                    :label="`${t('heatmaps.canvas', { path: selected })}. ${t('heatmaps.outline')}`"
                  />
                </div>
              </div>
              <table class="w-full table-fixed text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider">
                <caption class="sr-only">{{ t('heatmaps.topCells') }}</caption>
                <thead class="bg-fill">
                  <tr>
                    <th
                      scope="col"
                      class="h-table-head px-4 text-left text-[0.875rem] font-medium text-muted"
                    >
                      {{ t('heatmaps.topCells') }}
                    </th>
                    <th
                      scope="col"
                      class="h-table-head w-28 px-4 text-right text-[0.875rem] font-medium text-muted"
                    >
                      {{ t('heatmaps.clicks') }}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <tr
                    v-for="cell in topCells"
                    :key="`${cell.column}-${cell.row}`"
                    class="border-t border-divider first:border-t-0"
                  >
                    <th
                      scope="row"
                      class="h-table-row px-4 text-left font-normal text-foreground"
                    >
                      {{
                        t('heatmaps.cellPosition', {
                          x: percent(cell.column + 0.5, cells.data.value.columns),
                          y: format.integer(cell.row * cells.data.value.row_height),
                        })
                      }}
                    </th>
                    <td class="h-table-row px-4 text-right font-medium text-primary tabular-nums">{{ format.integer(cell.count) }}</td>
                  </tr>
                </tbody>
              </table>
            </template>
          </TabsContent>
          <TabsContent
            value="scroll"
            class="mt-0"
          >
            <table
              v-if="cells.data.value"
              class="w-full text-sm [&_tbody_:is(th,td):not(:first-child)]:border-l [&_tbody_:is(th,td)]:border-divider"
            >
              <thead class="bg-fill">
                <tr>
                  <th
                    scope="col"
                    class="h-table-head w-20 px-4 text-left text-[0.875rem] font-medium text-muted"
                  >
                    {{ t('heatmaps.depth') }}
                  </th>
                  <th
                    scope="col"
                    class="h-table-head px-4 text-left text-[0.875rem] font-medium text-muted"
                  >
                    {{ t('heatmaps.reached') }}
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="depth in cells.data.value.scroll"
                  :key="depth.depth_percent"
                  class="border-t border-divider first:border-t-0"
                >
                  <th
                    scope="row"
                    class="h-table-row px-4 text-left font-normal text-foreground tabular-nums"
                  >
                    {{ format.percent(depth.depth_percent / 100) }}
                  </th>
                  <td class="h-table-row px-4">
                    <div class="flex items-center gap-3">
                      <div
                        class="h-2 flex-1 overflow-hidden rounded-full bg-fill-strong"
                        aria-hidden="true"
                      >
                        <div
                          class="h-full rounded-full bg-series-primary"
                          :style="{ width: `${share(depth.visits, cells.data.value.scroll_visits) * 100}%` }"
                        />
                      </div>
                      <span class="w-28 shrink-0 text-right tabular-nums">
                        <span class="font-medium text-primary">{{ format.integer(depth.visits) }}</span>
                        <span class="text-muted"> · {{ percent(depth.visits, cells.data.value.scroll_visits) }}</span>
                      </span>
                    </div>
                  </td>
                </tr>
              </tbody>
            </table>
          </TabsContent>
        </Panel>
      </Tabs>
    </div>
  </SiteBoundary>
</template>
