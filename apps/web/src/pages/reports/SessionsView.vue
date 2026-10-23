<script setup lang="ts">
import CountryLabel from '~/components/CountryLabel.vue'
import Icon from '~/components/Icon.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import PopulationNote from '~/components/report/PopulationNote.vue'
import ReportScreen from '~/components/report/ReportScreen.vue'
import { Button } from '~/components/ui/button'
import { Panel } from '~/components/ui/panel'
import {
  Table,
  TableBody,
  TableCell,
  TableGroupRow,
  TableHead,
  TableHeader,
  TableMeter,
  TableNone,
  TableRow,
} from '~/components/ui/table'
import { useExplorationFormat } from '~/composables/useExplorationFormat'
import { useExplorationScreen } from '~/composables/useExplorationScreen'
import { useFormat } from '~/composables/useFormat'
import type { Cell, Field, SessionColumn, SessionOrder, Site } from '~/generated'
import { valueIcon } from '~/lib/client-looks'
import { useI18n } from '~/plugins/i18n'
import { setPage } from '~/services/exploration'
import { sessionListState } from '~/services/reports'

/**
 * Sessions starting in range, with a filter those with at least one matching event: the
 * drill-down target of every report row. Paged on the server over one snapshot.
 */
const props = defineProps<{ site: Site }>()

const { t, locale } = useI18n()
const format = useExplorationFormat()
const numbers = useFormat()
const screen = useExplorationScreen(
  () => props.site.id,
  () => sessionListState(props.site.id, props.site.timezone)
)

const definition = computed(() => (screen.state.value?.report?.kind === 'session_list' ? screen.state.value.report : null))
const orders: { value: SessionOrder; label: string }[] = [
  { value: 'start_desc', label: t('reports.sessions.newest') },
  { value: 'start_asc', label: t('reports.sessions.oldest') },
]
const setOrder = (order: SessionOrder) => {
  const state = screen.state.value
  const report = definition.value
  if (state && report) void screen.commit({ ...state, report: { ...report, order }, page: { ...state.page, index: 0 } })
}

/** A row's cell for `column`, wherever the state lists it; saved views may list fewer columns. */
const cell = (row: Cell[], column: SessionColumn): Cell =>
  row[definition.value?.columns.indexOf(column) ?? -1] ?? { t: 'not_set' }
const text = (row: Cell[], column: SessionColumn): string | null => {
  const value = cell(row, column)
  return value.t === 'string' ? value.v : null
}
const integer = (row: Cell[], column: SessionColumn): number | null => {
  const value = cell(row, column)
  return value.t === 'integer' ? value.v : null
}

const started = (ms: number, timezone: string) =>
  new Intl.DateTimeFormat(locale.value, {
    timeZone: timezone,
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  }).format(ms)
const time = (ms: number, timezone: string) =>
  new Intl.DateTimeFormat(locale.value, { timeZone: timezone, hour: 'numeric', minute: '2-digit' }).format(ms)

/** The page's rows under one heading per calendar day in the site's timezone, in the list's order. */
const byDay = (rows: Cell[][], timezone: string) => {
  const key = new Intl.DateTimeFormat('en-CA', { timeZone: timezone, year: 'numeric', month: '2-digit', day: '2-digit' })
  const label = new Intl.DateTimeFormat(locale.value, { timeZone: timezone, weekday: 'long', month: 'short', day: 'numeric' })
  const days: { key: string; label: string; rows: Cell[][] }[] = []
  for (const row of rows) {
    const start = integer(row, 'start') ?? 0
    const day = key.format(start)
    const last = days.at(-1)
    if (last?.key === day) last.rows.push(row)
    else days.push({ key: day, label: label.format(start), rows: [row] })
  }
  return days
}

/** A span fills the meter at 30 minutes, so a bar means the same length on every page. */
const SPAN_SCALE_MS = 30 * 60 * 1000

const named = (field: Field, value: string | null) => (value ? format.textLabel(field, value) : null)
const source = (row: Cell[]) => text(row, 'referrer_domain') ?? named('channel', text(row, 'channel'))
const device = (row: Cell[]) => {
  const value = text(row, 'device')
  const icon = value ? valueIcon('device', value) : null
  return value && icon ? { icon, label: format.textLabel('device', value) } : null
}
const regionId = useId()
</script>

<template>
  <ReportScreen
    :site="site"
    :screen="screen"
    :title="t('reports.sessions.title')"
  >
    <template
      v-if="$slots.context"
      #context
    >
      <slot name="context" />
    </template>
    <template #result="{ state, report }">
      <Panel
        v-if="report.kind === 'session_list' && definition"
        :title="t('reports.sessions.table')"
        :meta="t('reports.sessions.count', { count: numbers.integer(report.row_count) })"
        flush
      >
        <template #actions>
          <ChoiceGroup
            :model-value="definition.order"
            :label="t('reports.sessions.order')"
            :options="orders"
            @update:model-value="setOrder"
          />
          <PopulationNote :notes="[{ scope: 'session', coverage: report.coverage }]" />
        </template>
        <div
          :id="regionId"
          role="region"
          tabindex="0"
          :aria-label="t('reports.sessions.table')"
          class="overflow-x-auto focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-ring"
        >
          <Table
            bare
            class="min-w-[54rem] table-fixed"
          >
            <caption class="sr-only">{{ t('reports.sessions.table') }}</caption>
            <colgroup>
              <col class="w-28" />
              <col />
              <col class="w-44" />
              <col class="w-48" />
              <col class="w-44" />
              <col class="w-20" />
              <col class="w-20" />
            </colgroup>
            <TableHeader>
              <TableRow>
                <TableHead :aria-sort="definition.order === 'start_asc' ? 'ascending' : 'descending'">
                  {{ t('reports.sessions.list.time') }}
                </TableHead>
                <TableHead>{{ t('reports.sessions.columns.session_id') }}</TableHead>
                <TableHead>{{ t('reports.sessions.list.location') }}</TableHead>
                <TableHead>{{ t('reports.sessions.list.device') }}</TableHead>
                <TableHead>{{ t('reports.sessions.columns.span') }}</TableHead>
                <TableHead numeric>{{ t('reports.sessions.columns.pageviews') }}</TableHead>
                <TableHead numeric>{{ t('reports.sessions.columns.custom_events') }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody
              v-for="day in byDay(report.rows, state.timezone)"
              :key="day.key"
            >
              <TableGroupRow :colspan="7">{{ day.label }}</TableGroupRow>
              <TableRow
                v-for="(row, index) in day.rows"
                :key="index"
                class="relative"
              >
                <TableCell
                  kind="secondary"
                  class="tabular-nums"
                >
                  <RouterLink
                    v-if="integer(row, 'session_id') !== null"
                    :to="{ name: 'session', params: { siteId: site.id, sessionId: integer(row, 'session_id') } }"
                    class="after:absolute after:inset-0 focus-visible:outline-none focus-visible:after:outline-2 focus-visible:after:-outline-offset-2 focus-visible:after:outline-ring"
                    :aria-label="t('reports.sessions.list.open', { time: started(integer(row, 'start') ?? 0, state.timezone) })"
                  >
                    {{ time(integer(row, 'start') ?? 0, state.timezone) }}
                  </RouterLink>
                  <template v-else>{{ time(integer(row, 'start') ?? 0, state.timezone) }}</template>
                </TableCell>
                <TableCell kind="subject">
                  <span class="flex min-w-0 items-center gap-2">
                    <span class="truncate">
                      <template v-if="text(row, 'entry_path')">{{ text(row, 'entry_path') }}</template>
                      <TableNone v-else />
                    </span>
                    <span
                      v-if="text(row, 'identity_kind') === 'person'"
                      class="shrink-0 rounded-sm bg-fill-strong px-1.5 text-2xs font-medium text-foreground"
                    >
                      {{ t('reports.sessions.list.person') }}
                    </span>
                  </span>
                  <template #detail>
                    <template v-if="source(row)">{{ source(row) }}</template>
                    <TableNone v-else />
                  </template>
                </TableCell>
                <TableCell>
                  <CountryLabel
                    v-if="text(row, 'country')"
                    :code="text(row, 'country') ?? ''"
                    class="max-w-full"
                  />
                  <TableNone v-else />
                  <template #detail>
                    <span :class="text(row, 'country') && 'pl-[26px]'">
                      <template v-if="text(row, 'city')">{{ text(row, 'city') }}</template>
                      <TableNone v-else />
                    </span>
                  </template>
                </TableCell>
                <TableCell>
                  <span class="flex min-w-0 items-center gap-2">
                    <Icon
                      v-if="device(row)"
                      :name="device(row)?.icon ?? ''"
                      size="15"
                      class="shrink-0 text-muted"
                      :aria-label="device(row)?.label"
                    />
                    <span class="truncate">{{ named('browser', text(row, 'browser')) }}</span>
                  </span>
                  <template #detail>
                    <span :class="device(row) && 'pl-[23px]'">{{ named('os', text(row, 'os')) }}</span>
                  </template>
                </TableCell>
                <TableCell class="tabular-nums">
                  <span class="flex items-center gap-3">
                    <span class="w-16 text-right text-primary">
                      <template v-if="integer(row, 'span')">{{ numbers.duration((integer(row, 'span') ?? 0) / 1000) }}</template>
                      <TableNone v-else />
                    </span>
                    <TableMeter
                      v-if="integer(row, 'span')"
                      :value="(integer(row, 'span') ?? 0) / SPAN_SCALE_MS"
                    />
                  </span>
                </TableCell>
                <TableCell kind="numeric">
                  {{ numbers.integer(integer(row, 'pageviews') ?? 0) }}
                </TableCell>
                <TableCell kind="numeric">
                  <template v-if="integer(row, 'custom_events')">
                    {{ numbers.integer(integer(row, 'custom_events') ?? 0) }}
                  </template>
                  <TableNone v-else />
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </div>
        <div
          v-if="report.row_count > state.page.size"
          class="flex items-center justify-end gap-2 border-t border-divider px-4 py-2 text-xs text-muted"
        >
          <Button
            variant="outline"
            size="xs"
            :disabled="state.page.index === 0"
            :aria-label="t('explore.table.previousPage')"
            @click="screen.commit(setPage(state, state.page.index - 1))"
          >
            <Icon name="lucide:chevron-left" />
          </Button>
          <span class="tabular-nums">
            {{ t('explore.table.pageOf', { page: state.page.index + 1, pages: Math.ceil(report.row_count / state.page.size) }) }}
          </span>
          <Button
            variant="outline"
            size="xs"
            :disabled="(state.page.index + 1) * state.page.size >= report.row_count"
            :aria-label="t('explore.table.nextPage')"
            @click="screen.commit(setPage(state, state.page.index + 1))"
          >
            <Icon name="lucide:chevron-right" />
          </Button>
        </div>
      </Panel>
    </template>
  </ReportScreen>
</template>
