<script setup lang="ts">
import StorageChart from '~/components/admin/StorageChart.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Badge } from '~/components/ui/badge'
import { Panel } from '~/components/ui/panel'
import { Progress } from '~/components/ui/progress'
import { Skeleton } from '~/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import TableEmptyRow from '~/components/ui/TableEmptyRow.vue'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useDateFormat } from '~/composables/useDateFormat'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import { useOperatorStatus } from '~/composables/useOperatorStatus'
import type { MaintenanceOutcome } from '~/generated'
import {
  blocksCollection,
  problemDescriptions,
  problemTitles,
  share,
  stateLabels,
  taskLabels,
  useOperatorStatusText,
  usedBytes,
} from '~/lib/operator-status'
import { useI18n } from '~/plugins/i18n'

/**
 * What an install admin needs instead of a CLI or `/health/ready`: the server state in words,
 * disk use with the storage forecast and what it was computed from, the disk and recording
 * budgets, and the latest maintenance outcomes. The router shows it to install admins only; the
 * server refuses everyone else.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const { dateTime } = useDateFormat()
const format = useFormat()
const text = useOperatorStatusText()
const { query } = useOperatorStatus()

const status = computed(() => query.data.value)
const stopped = computed(() => status.value?.problems.some(blocksCollection) ?? false)

const disk = computed(() => {
  if (!status.value) return null
  const used = usedBytes(status.value.disk)
  const free = status.value.disk.free_bytes
  return { used, free, reserve: status.value.collection.disk_reserve_bytes }
})

const inputs = computed(() => {
  const forecast = status.value?.forecast
  if (!forecast) return []
  const i = forecast.inputs
  const unknown = t('admin.status.forecast.unknown')
  return [
    { key: 'sampledAt', label: t('admin.status.forecast.sampledAt'), value: dateTime(i.sampled_at) },
    { key: 'retained', label: t('admin.status.forecast.retainedRecords'), value: format.integer(i.retained_records) },
    { key: 'perRecord', label: t('admin.status.forecast.bytesPerRecord'), value: format.integer(i.bytes_per_record) },
    { key: 'perDay', label: t('admin.status.forecast.ingestPerDay'), value: format.integer(i.ingest_per_day) },
    { key: 'oldest', label: t('admin.status.forecast.oldestDay'), value: i.oldest_day ? text.day(i.oldest_day) : unknown },
    {
      key: 'full',
      label: t('admin.status.forecast.fullRetentionOn'),
      value: i.full_retention_on ? text.day(i.full_retention_on) : unknown,
    },
    { key: 'sqlite', label: t('admin.status.forecast.sqlite'), value: text.bytes(i.sqlite_bytes) },
    { key: 'cap', label: t('admin.status.forecast.replayCap'), value: text.bytes(i.replay_cap_bytes) },
    { key: 'reserve', label: t('admin.status.forecast.reserve'), value: text.bytes(i.disk_reserve_bytes) },
    { key: 'steady', label: t('admin.status.forecast.steadyState'), value: text.bytes(i.steady_state_bytes) },
    { key: 'used', label: t('admin.status.forecast.used'), value: text.bytes(i.used_bytes) },
    { key: 'capacity', label: t('admin.status.forecast.capacity'), value: text.bytes(i.capacity_bytes) },
  ]
})

const result = (outcome: MaintenanceOutcome) =>
  outcome.state === 'failed' && outcome.error_code
    ? t('admin.status.maintenance.failedWith', { code: outcome.error_code })
    : t(stateLabels[outcome.state])
</script>

<template>
  <Skeleton
    v-if="query.isPending.value"
    class="h-96 w-full"
  />
  <p
    v-else-if="query.isError.value"
    class="text-sm text-destructive"
  >
    {{ message(query.error.value) }}
  </p>
  <div
    v-else-if="status && disk"
    class="grid gap-6"
  >
    <Panel>
      <div class="flex flex-wrap items-center gap-x-6 gap-y-4">
        <div class="flex min-w-0 items-center gap-2.5">
          <span
            class="size-2.5 shrink-0 rounded-full"
            :class="stopped ? 'bg-destructive' : status.problems.length > 0 ? 'bg-warning' : 'bg-success'"
            aria-hidden="true"
          />
          <h2 class="truncate text-lg font-semibold text-primary">{{ text.state(status.problems) }}</h2>
          <Badge
            v-if="status.collection.clock === 'test'"
            variant="warning"
            size="sm"
          >
            {{ t('admin.status.testClock') }}
          </Badge>
        </div>
        <dl class="flex flex-wrap gap-x-8 gap-y-2 text-sm sm:ml-auto">
          <div class="flex items-baseline gap-2">
            <dt class="text-muted">{{ t('admin.status.lag') }}</dt>
            <dd class="font-medium text-primary tabular-nums">
              {{ format.duration(status.collection.processing_lag_ms / 1000) }}
            </dd>
          </div>
          <div class="flex items-baseline gap-2">
            <dt class="text-muted">{{ t('admin.status.waiting') }}</dt>
            <dd class="font-medium text-primary tabular-nums">
              {{
                t('admin.status.waitingValue', {
                  count: format.integer(status.collection.inbox_records),
                  size: text.bytes(status.collection.inbox_bytes),
                })
              }}
            </dd>
          </div>
        </dl>
      </div>
      <div
        v-if="status.problems.length > 0"
        class="mt-4 grid gap-2"
      >
        <p class="text-sm text-foreground">
          {{ stopped ? t('admin.status.collectionStopped') : t('admin.status.collectionReady') }}
        </p>
        <ul class="grid gap-2">
          <li
            v-for="problem in status.problems"
            :key="problem"
          >
            <Alert
              :color="blocksCollection(problem) ? 'destructive' : 'warning'"
              icon="lucide:triangle-alert"
            >
              <p class="text-sm font-medium">{{ t(problemTitles[problem]) }}</p>
              <p class="text-sm text-foreground">{{ t(problemDescriptions[problem]) }}</p>
            </Alert>
          </li>
        </ul>
      </div>
    </Panel>

    <div class="grid gap-6 xl:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
      <Panel
        id="storage-forecast"
        :title="t('admin.status.forecast.title')"
      >
        <div class="grid gap-4">
          <p class="text-base text-primary">{{ text.forecast(status.forecast) }}</p>
          <StorageChart
            v-if="status.samples.length > 0"
            :samples="status.samples"
            :forecast="status.forecast"
          />
          <details
            v-if="inputs.length > 0"
            class="text-sm"
          >
            <summary class="cursor-pointer font-medium text-primary">{{ t('admin.status.forecast.inputs') }}</summary>
            <p class="mt-2 text-muted">{{ t('admin.status.forecast.formula') }}</p>
            <dl class="mt-3 grid gap-x-8 gap-y-2 sm:grid-cols-2">
              <div
                v-for="input in inputs"
                :key="input.key"
                class="flex justify-between gap-4 border-b border-divider pb-1"
              >
                <dt class="text-muted">{{ input.label }}</dt>
                <dd class="text-right font-medium text-primary tabular-nums">{{ input.value }}</dd>
              </div>
            </dl>
          </details>
        </div>
      </Panel>

      <Panel
        id="budgets"
        :title="t('admin.status.budgets.title')"
      >
        <div class="grid content-start gap-5">
          <div class="grid gap-2">
            <p class="flex flex-wrap justify-between gap-x-4 text-sm">
              <span class="text-muted">{{ t('admin.status.budgets.disk') }}</span>
              <span class="font-medium text-primary tabular-nums">
                {{ t('admin.status.budgets.diskUsed', { used: text.bytes(disk.used), free: text.bytes(disk.free) }) }}
              </span>
            </p>
            <Progress
              :model-value="share(disk.used, disk.used + disk.free)"
              :variant="status.problems.includes('disk_full') ? 'destructive' : 'default'"
              :aria-label="t('admin.status.budgets.disk')"
            />
            <p class="text-xs text-muted">{{ t('admin.status.budgets.diskReserve', { reserve: text.bytes(disk.reserve) }) }}</p>
          </div>
          <div class="grid gap-2">
            <p class="flex flex-wrap justify-between gap-x-4 text-sm">
              <span class="text-muted">{{ t('admin.status.budgets.replay') }}</span>
              <span class="font-medium text-primary tabular-nums">
                {{
                  t('admin.status.budgets.ofCap', {
                    used: text.bytes(status.recording.used_bytes),
                    cap: text.bytes(status.recording.cap_bytes),
                  })
                }}
              </span>
            </p>
            <Progress
              :model-value="share(status.recording.used_bytes, status.recording.cap_bytes)"
              :variant="status.problems.includes('recorder_budget') ? 'warning' : 'default'"
              :aria-label="t('admin.status.budgets.replay')"
            />
          </div>
          <div class="grid gap-2">
            <p class="flex flex-wrap justify-between gap-x-4 text-sm">
              <span class="text-muted">{{ t('admin.status.budgets.saved') }}</span>
              <span class="font-medium text-primary tabular-nums">
                {{
                  t('admin.status.budgets.ofCap', {
                    used: text.bytes(status.recording.saved_bytes),
                    cap: text.bytes(status.recording.saved_cap_bytes),
                  })
                }}
              </span>
            </p>
            <Progress
              :model-value="share(status.recording.saved_bytes, status.recording.saved_cap_bytes)"
              :aria-label="t('admin.status.budgets.saved')"
            />
          </div>
        </div>
      </Panel>
    </div>

    <section
      class="grid gap-3"
      aria-labelledby="maintenance-title"
    >
      <h2
        id="maintenance-title"
        class="flex items-center gap-1 text-[1.0625rem] font-semibold text-primary"
      >
        {{ t('admin.status.maintenance.title') }}
        <SimpleTooltip
          :tooltip="t('admin.status.maintenance.description')"
          :aria-label="t('admin.status.maintenance.description')"
          class="inline-flex size-6 cursor-help items-center justify-center rounded-control text-muted hover:text-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          <Icon
            name="lucide:info"
            size="14"
            aria-hidden="true"
          />
        </SimpleTooltip>
      </h2>
      <Table :label="t('admin.status.maintenance.title')">
        <TableHeader>
          <TableRow>
            <TableHead>{{ t('admin.status.maintenance.task') }}</TableHead>
            <TableHead>{{ t('admin.status.maintenance.result') }}</TableHead>
            <TableHead>{{ t('admin.status.maintenance.started') }}</TableHead>
            <TableHead>{{ t('admin.status.maintenance.finished') }}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableEmptyRow
            v-if="status.maintenance.length === 0"
            :colspan="4"
            :label="t('admin.status.maintenance.empty')"
          />
          <TableRow
            v-for="outcome in status.maintenance"
            :key="outcome.task"
          >
            <TableCell class="font-medium text-primary">{{ t(taskLabels[outcome.task]) }}</TableCell>
            <TableCell>
              <Badge
                :variant="outcome.state === 'failed' ? 'destructive' : outcome.state === 'done' ? 'success' : 'secondary'"
                size="sm"
              >
                {{ result(outcome) }}
              </Badge>
            </TableCell>
            <TableCell class="text-muted tabular-nums">{{ outcome.started_at ? dateTime(outcome.started_at) : '' }}</TableCell>
            <TableCell class="text-muted tabular-nums">{{ outcome.finished_at ? dateTime(outcome.finished_at) : '' }}</TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </section>
  </div>
</template>
