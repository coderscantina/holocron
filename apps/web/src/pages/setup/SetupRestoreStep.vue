<script setup lang="ts">
import { setupResource } from '~/api/resources/setup'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { Spinner } from '~/components/ui/spinner'
import type { SetupRestorePhase, SetupRestoreProgress, SetupStepState } from '~/generated'
import { type MessageKey, useI18n } from '~/plugins/i18n'
import { serverStarted } from '~/services/runtime-config'

import { restoreRows } from './restore'

/**
 * The running restore, phase by phase, from the server's progress stream. A reload attaches
 * again and gets the current state first. Emits `done` once the server left setup mode, and
 * `retry` when the restore stopped and the person wants another backup.
 */
const props = defineProps<{ token: string }>()
const emit = defineEmits<{ done: []; retry: [] }>()

const { t } = useI18n()

const names: Record<SetupRestorePhase, MessageKey> = {
  restoring: 'setup.restore.phases.restoring',
  upgrading: 'setup.restore.phases.upgrading',
}
const icons: Record<SetupStepState, string> = {
  pending: 'lucide:circle-dashed',
  running: '',
  done: 'lucide:circle-check',
}

const progress = shallowRef<SetupRestoreProgress | null>(null)
/** A dropped stream reconnects after this long, as the server may still be running the job. */
const RECONNECT_MS = 2000
let close: (() => void) | null = null
let retry: ReturnType<typeof setTimeout> | null = null
let finished = false

async function finish(): Promise<void> {
  if (finished) return
  finished = true
  // Setup mode ends right after the last frame; the server opens its database first.
  await serverStarted()
  emit('done')
}

function connect(): void {
  close = setupResource.restoreProgress(
    props.token,
    (frame) => {
      progress.value = frame
      if (frame.state === 'complete') void finish()
    },
    () => {
      if (progress.value?.state === 'complete') return
      retry = setTimeout(connect, RECONNECT_MS)
    }
  )
}
onMounted(connect)
onBeforeUnmount(() => {
  close?.()
  if (retry) clearTimeout(retry)
})

const rows = computed(() => (progress.value ? restoreRows(progress.value) : []))
</script>

<template>
  <div
    class="grid gap-7"
    data-wide
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.restore.title') }}
      <span class="block font-medium text-muted">{{ t('setup.restore.subtitle') }}</span>
    </h1>
    <ol
      v-if="rows.length > 0 && progress?.state !== 'failed'"
      class="grid gap-2"
      aria-live="polite"
    >
      <li
        v-for="row in rows"
        :key="row.phase"
        class="flex items-center gap-3 rounded-[var(--radius-control)] bg-fill p-4 text-sm"
      >
        <Spinner
          v-if="row.state === 'running'"
          class="size-4 shrink-0 text-accent-text"
        />
        <Icon
          v-else
          :name="icons[row.state]"
          class="size-4 shrink-0"
          :class="row.state === 'done' ? 'text-success' : 'text-muted'"
          aria-hidden="true"
        />
        <span class="font-medium text-primary">{{ t(names[row.phase]) }}</span>
      </li>
    </ol>
    <template v-else-if="progress?.state === 'failed'">
      <Alert color="destructive">
        <div class="grid gap-1 text-sm">
          <p class="font-medium">{{ t('setup.restore.failed') }}</p>
          <p
            v-if="progress.error"
            class="break-words whitespace-pre-wrap"
          >
            {{ progress.error.message }}
          </p>
        </div>
      </Alert>
      <Button
        variant="accent"
        size="lg"
        @click="emit('retry')"
      >
        {{ t('setup.restore.retry') }}
      </Button>
    </template>
    <p
      v-else
      class="flex items-center gap-3 text-sm text-muted"
    >
      <Spinner class="size-4" />
      {{ t('setup.restore.connecting') }}
    </p>
  </div>
</template>
