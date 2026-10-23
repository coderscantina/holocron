<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Label } from '~/components/ui/label'
import { Spinner } from '~/components/ui/spinner'
import { Switch } from '~/components/ui/switch'
import { useRecordingFormat } from '~/composables/useRecordingFormat'
import { parseFromFrame, SPEEDS, type ToFrame } from '~/lib/replay-frame'
import { useI18n } from '~/plugins/i18n'

/**
 * Plays a replay in the replay frame, which renders captured pages without scripts, pointer
 * input or network access. The frame gets each page's event JSON; this component only sends
 * commands and shows what the frame reports.
 */
const props = defineProps<{
  /** Event JSON of each loaded playback page, in order. */
  pages: string[]
  /** More pages are on their way. */
  loading: boolean
}>()

const emit = defineEmits<{ start: [at: number]; time: [at: number] }>()

const { t } = useI18n()
const format = useRecordingFormat()

const frame = ref<HTMLIFrameElement | null>(null)
const ready = ref(false)
const failed = ref(false)
const sent = ref(0)
const playing = ref(false)
const position = ref(0)
const duration = ref(0)
const start = ref(0)
const aspect = ref(16 / 10)
const speed = ref<(typeof SPEEDS)[number]>(1)
const speedOptions = SPEEDS.map((value) => ({ value, label: `${value}×` }))
const skip = ref(true)

function post(message: ToFrame) {
  frame.value?.contentWindow?.postMessage(message, location.origin)
}

/** Hands over pages the frame has not seen yet. */
function flush() {
  if (!ready.value) return
  while (sent.value < props.pages.length) {
    const events = props.pages[sent.value] ?? '[]'
    post(sent.value === 0 ? { type: 'load', events } : { type: 'append', events })
    sent.value += 1
  }
}

function onMessage(event: MessageEvent) {
  if (!frame.value || event.source !== frame.value.contentWindow || event.origin !== location.origin) return
  const message = parseFromFrame(event.data)
  if (!message) return
  switch (message.type) {
    case 'ready':
      ready.value = true
      sent.value = 0
      post({ type: 'skip', skip: skip.value })
      return flush()
    case 'loaded':
      start.value = message.start
      emit('start', message.start)
      duration.value = message.duration
      if (message.width > 0 && message.height > 0) aspect.value = message.width / message.height
      return
    case 'time':
      position.value = message.at
      playing.value = message.playing
      return emit('time', start.value + message.at)
    case 'failed':
      failed.value = true
  }
}

onMounted(() => window.addEventListener('message', onMessage))
onBeforeUnmount(() => window.removeEventListener('message', onMessage))
watch(() => props.pages.length, flush)
watch(speed, (value) => post({ type: 'speed', speed: value }))
watch(skip, (value) => post({ type: 'skip', skip: value }))

const toggle = () => post({ type: playing.value ? 'pause' : 'play' })

/** Seeks to a browser time, as the timeline lists them. */
function seekTo(at: number) {
  const offset = Math.min(Math.max(0, at - start.value), duration.value)
  position.value = offset
  post({ type: 'seek', at: offset })
}

const onSlider = (event: Event) => {
  if (!(event.target instanceof HTMLInputElement)) return
  position.value = Number(event.target.value)
  post({ type: 'seek', at: position.value })
}

defineExpose({ seekTo })
</script>

<template>
  <div class="grid gap-3">
    <EmptyState
      v-if="failed"
      icon="lucide:film"
      :title="t('replays.player.unavailable.title')"
      :description="t('replays.player.unavailable.description')"
    />
    <div
      v-else
      class="relative w-full overflow-hidden rounded-lg border border-hairline bg-fill"
      :style="{ aspectRatio: String(aspect), maxHeight: '70vh' }"
    >
      <iframe
        ref="frame"
        src="/replay-frame.html"
        sandbox="allow-scripts allow-same-origin"
        referrerpolicy="no-referrer"
        :title="t('replays.player.frame')"
        class="absolute inset-0 size-full border-0"
      />
      <div
        v-if="!ready || duration === 0"
        class="absolute inset-0 grid place-items-center"
        role="status"
      >
        <span class="flex items-center gap-2 text-sm text-muted">
          <Spinner />
          {{ t('replays.player.loading') }}
        </span>
      </div>
    </div>

    <div
      v-if="!failed"
      class="flex flex-wrap items-center gap-x-4 gap-y-2"
    >
      <Button
        size="sm"
        variant="accent"
        :disabled="duration === 0"
        :aria-label="playing ? t('replays.player.pause') : t('replays.player.play')"
        @click="toggle"
      >
        <Icon
          :name="playing ? 'lucide:pause' : 'lucide:play'"
          aria-hidden="true"
        />
        {{ playing ? t('replays.player.pause') : t('replays.player.play') }}
      </Button>
      <label class="flex min-w-48 flex-1 items-center gap-3">
        <span class="sr-only">{{ t('replays.player.position') }}</span>
        <input
          type="range"
          class="h-2 flex-1 cursor-pointer accent-accent"
          min="0"
          :max="duration"
          step="100"
          :value="position"
          :disabled="duration === 0"
          :aria-valuetext="t('replays.player.time', { current: format.clock(position), total: format.clock(duration) })"
          @input="onSlider"
        >
        <span class="shrink-0 text-sm text-muted tabular-nums">
          {{ t('replays.player.time', { current: format.clock(position), total: format.clock(duration) }) }}
        </span>
      </label>
      <ChoiceGroup
        v-model="speed"
        :label="t('replays.player.speed')"
        :options="speedOptions"
      />
      <div class="flex items-center gap-2">
        <Switch
          id="replay-skip"
          v-model="skip"
        />
        <Label for="replay-skip">{{ t('replays.player.skip') }}</Label>
      </div>
      <span
        v-if="loading"
        class="flex items-center gap-2 text-xs text-muted"
        role="status"
      >
        <Spinner />
        {{ t('replays.player.loadingMore') }}
      </span>
    </div>
  </div>
</template>
