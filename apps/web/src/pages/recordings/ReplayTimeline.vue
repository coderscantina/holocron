<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import type { TimelineEntry, TimelineKind } from '~/generated'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * The replay's events as a list a keyboard and a screen reader can use. Each entry seeks the
 * player; the entry at the playback position is marked current. Typed text is never listed.
 */
const props = defineProps<{
  entries: TimelineEntry[]
  /** Browser time of the first event and of the playback position. */
  start: number
  current: number
}>()

const emit = defineEmits<{ seek: [at: number] }>()

const { t } = useI18n()
const format = useRecordingFormat()

const labels: Record<TimelineKind, MessageKey> = {
  page: 'replays.timeline.page',
  click: 'replays.timeline.click',
  input: 'replays.timeline.input',
  scroll: 'replays.timeline.scroll',
  resize: 'replays.timeline.resize',
}
const icons: Record<TimelineKind, string> = {
  page: 'lucide:file',
  click: 'lucide:mouse-pointer-click',
  input: 'lucide:keyboard',
  scroll: 'lucide:arrow-down-up',
  resize: 'lucide:scaling',
}

const currentIndex = computed(() => props.entries.findLastIndex((entry) => entry.at <= props.current))
</script>

<template>
  <ol
    v-if="entries.length > 0"
    class="grid gap-0.5"
  >
    <li
      v-for="(entry, index) in entries"
      :key="`${entry.at}-${index}`"
    >
      <button
        type="button"
        class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors hover:bg-fill focus-visible:outline-2 focus-visible:outline-ring aria-[current=step]:bg-fill-strong"
        :aria-current="index === currentIndex ? 'step' : undefined"
        :aria-label="`${t(labels[entry.kind], { path: entry.path ?? '' })}, ${t('replays.timeline.jump', { time: format.clock(entry.at - start) })}`"
        @click="emit('seek', entry.at)"
      >
        <Icon
          :name="icons[entry.kind]"
          class="shrink-0 text-muted"
          aria-hidden="true"
        />
        <span class="min-w-0 flex-1 truncate text-primary">{{ t(labels[entry.kind], { path: entry.path ?? '' }) }}</span>
        <span class="shrink-0 text-xs text-muted tabular-nums">{{ format.clock(entry.at - start) }}</span>
      </button>
    </li>
  </ol>
  <p
    v-else
    class="text-sm text-muted"
  >
    {{ t('replays.timeline.empty') }}
  </p>
</template>
