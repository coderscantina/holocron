<script setup lang="ts">
import type { RouteLocationRaw } from 'vue-router'

import CountryLabel from '~/components/CountryLabel.vue'
import Icon from '~/components/Icon.vue'
import { Badge } from '~/components/ui/badge'
import { Button } from '~/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '~/components/ui/table'
import { useFormat } from '~/composables/useFormat'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import type { CaptureSummary } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/** Replays as a table, newest first, each linking to its player. */
defineProps<{
  captures: CaptureSummary[]
  link: (capture: CaptureSummary) => RouteLocationRaw
  /** More pages wait on the server. */
  more: boolean
  loadingMore: boolean
}>()
const emit = defineEmits<{ more: [] }>()

const { t } = useI18n()
const format = useFormat()
const recordingFormat = useRecordingFormat()
</script>

<template>
  <div class="grid gap-3">
    <Table
      class="table-fixed"
      :label="t('replays.listLabel')"
    >
      <TableHeader>
        <TableRow>
          <TableHead>{{ t('replays.columns.recorded') }}</TableHead>
          <TableHead class="w-28 text-right">{{ t('replays.columns.duration') }}</TableHead>
          <TableHead class="max-md:hidden">{{ t('replays.columns.entry') }}</TableHead>
          <TableHead class="w-24 text-right max-lg:hidden">{{ t('replays.columns.pages') }}</TableHead>
          <TableHead class="w-48 max-lg:hidden">{{ t('replays.columns.device') }}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        <TableRow
          v-for="capture in captures"
          :key="capture.id"
        >
          <th
            scope="row"
            class="h-table-row px-4 text-left font-normal"
          >
            <span class="flex min-w-0 items-center gap-2">
              <RouterLink
                :to="link(capture)"
                class="inline-flex min-w-0 items-center gap-2 font-medium text-primary hover:underline focus-visible:outline-2 focus-visible:outline-ring"
                :aria-label="t('replays.open', { time: recordingFormat.dateTime(capture.received_at) })"
              >
                <Icon
                  name="lucide:circle-play"
                  class="shrink-0 text-muted"
                  aria-hidden="true"
                />
                <span class="truncate">{{ recordingFormat.dateTime(capture.received_at) }}</span>
              </RouterLink>
              <Badge
                v-if="capture.saved"
                size="xs"
                variant="info"
              >
                {{ t('replays.saved') }}
              </Badge>
              <Badge
                v-if="capture.truncated"
                size="xs"
                variant="warning"
              >
                {{ t('replays.truncated') }}
              </Badge>
            </span>
          </th>
          <TableCell class="text-right text-foreground tabular-nums">
            {{ format.duration(capture.duration_ms / 1000) }}
          </TableCell>
          <TableCell
            class="truncate text-foreground max-md:hidden"
            :title="capture.entry_path"
          >
            {{ capture.entry_path ?? '' }}
          </TableCell>
          <TableCell class="text-right text-foreground tabular-nums max-lg:hidden">
            {{ format.integer(capture.pages) }}
          </TableCell>
          <TableCell class="text-muted max-lg:hidden">
            <span class="flex min-w-0 items-center gap-1.5">
              <CountryLabel
                v-if="capture.country"
                :code="capture.country"
                class="shrink-0"
              />
              <span
                v-if="capture.country && (capture.device || capture.browser)"
                aria-hidden="true"
              >·</span>
              <span class="truncate">{{ [capture.device, capture.browser].filter(Boolean).join(' · ') }}</span>
            </span>
          </TableCell>
        </TableRow>
      </TableBody>
    </Table>
    <div v-if="more">
      <Button
        variant="fill"
        size="sm"
        :loading="loadingMore"
        @click="emit('more')"
      >
        {{ t('replays.more') }}
      </Button>
    </div>
  </div>
</template>
