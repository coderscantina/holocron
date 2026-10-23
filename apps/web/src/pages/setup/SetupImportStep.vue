<script setup lang="ts">
import { setupResource } from '~/api/resources/setup'
import Icon from '~/components/Icon.vue'
import { Progress } from '~/components/ui/progress'
import { Spinner } from '~/components/ui/spinner'
import { useFormat } from '~/composables/useFormat'
import type { SetupImportProgress, SetupImportStepName, SetupStepState } from '~/generated'
import { type MessageKey, useI18n } from '~/plugins/i18n'

/**
 * The running import, step by step, from the server's progress stream. A reload attaches again
 * and gets the current state first. Emits `settled` once the run is no longer running.
 */
const emit = defineEmits<{ settled: [] }>()

const { t } = useI18n()
const { integer, duration } = useFormat()

const names: Record<SetupImportStepName, MessageKey> = {
  control: 'setup.import.steps.control',
  events: 'setup.import.steps.events',
  identify: 'setup.import.steps.identify',
  heatmaps: 'setup.import.steps.heatmaps',
  session_links: 'setup.import.steps.sessionLinks',
  replays: 'setup.import.steps.replays',
  finishing: 'setup.import.steps.finishing',
}
const icons: Record<SetupStepState, string> = {
  pending: 'lucide:circle-dashed',
  running: '',
  done: 'lucide:circle-check',
}

const progress = shallowRef<SetupImportProgress | null>(null)
/** A dropped stream reconnects after this long, as the server may still be running the job. */
const RECONNECT_MS = 2000
let close: (() => void) | null = null
let retry: ReturnType<typeof setTimeout> | null = null

function connect(): void {
  close = setupResource.progress(
    (frame) => {
      progress.value = frame
      if (frame.state !== 'running') emit('settled')
    },
    () => {
      retry = setTimeout(connect, RECONNECT_MS)
    }
  )
}
onMounted(connect)
onBeforeUnmount(() => {
  close?.()
  if (retry) clearTimeout(retry)
})

const percent = (rows: number, total: number) => (total > 0 ? (rows / total) * 100 : 0)
</script>

<template>
  <div
    class="grid gap-7"
    data-wide
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.import.title') }}
      <span class="block font-medium text-muted">{{ t('setup.import.subtitle') }}</span>
    </h1>
    <ol
      v-if="progress && progress.steps.length > 0"
      class="grid gap-2"
      aria-live="polite"
    >
      <li
        v-for="step in progress.steps"
        :key="step.step"
        class="grid gap-2 rounded-[var(--radius-control)] bg-fill p-4"
      >
        <div class="flex items-center gap-3 text-sm">
          <Spinner
            v-if="step.state === 'running'"
            class="size-4 shrink-0 text-accent-text"
          />
          <Icon
            v-else
            :name="icons[step.state]"
            class="size-4 shrink-0"
            :class="step.state === 'done' ? 'text-success' : 'text-muted'"
            aria-hidden="true"
          />
          <span class="font-medium text-primary">{{ t(names[step.step]) }}</span>
          <span
            v-if="step.total > 0"
            class="ml-auto text-muted tabular-nums"
          >
            {{ t('setup.import.rows', { done: integer(Math.min(step.rows, step.total)), total: integer(step.total) }) }}
          </span>
        </div>
        <template v-if="step.total > 0 && step.state === 'running'">
          <Progress
            :model-value="percent(step.rows, step.total)"
            class="h-1.5 bg-fill-strong"
          />
          <p
            v-if="step.eta_seconds !== undefined"
            class="text-xs text-muted"
          >
            {{ t('setup.import.eta', { time: duration(step.eta_seconds) }) }}
          </p>
        </template>
      </li>
    </ol>
    <p
      v-else
      class="flex items-center gap-3 text-sm text-muted"
    >
      <Spinner class="size-4" />
      {{ t('setup.import.connecting') }}
    </p>
  </div>
</template>
