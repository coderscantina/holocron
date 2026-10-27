<script setup lang="ts">
import { setupResource } from '~/api/resources/setup'
import SetupStepBar from '~/components/setup/SetupStepBar.vue'
import { usePageMeta } from '~/composables/usePageMeta'
import type { SetupBackup, SetupCheck, SetupImportReport, SetupInspection, SetupStatus } from '~/generated'
import { useI18n } from '~/plugins/i18n'
import { runtimeConfig } from '~/services/runtime-config'

import SetupAdminStep from './SetupAdminStep.vue'
import SetupBackupStep from './SetupBackupStep.vue'
import SetupCheckStep from './SetupCheckStep.vue'
import SetupConfirmStep from './SetupConfirmStep.vue'
import SetupImportStep from './SetupImportStep.vue'
import SetupOptionsStep from './SetupOptionsStep.vue'
import SetupPathStep from './SetupPathStep.vue'
import SetupReportStep from './SetupReportStep.vue'
import SetupRestoreStep from './SetupRestoreStep.vue'
import SetupSignInStep from './SetupSignInStep.vue'
import SetupSiteStep from './SetupSiteStep.vue'
import SetupSkipImport from './SetupSkipImport.vue'
import SetupSourceStep from './SetupSourceStep.vue'
import SetupTokenStep from './SetupTokenStep.vue'
import { setupSteps, type SetupPath, type SetupStep } from './steps'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()

usePageMeta(() => t('setup.title'))

/** Held in memory only: a reload starts again at the token. */
const token = ref('')
const check = shallowRef<SetupCheck | null>(null)
const path = ref<SetupPath>('fresh')
/** The Umami database URL, never stored or put in the address. A reload asks for it again. */
const source = ref('')
const inspection = shallowRef<SetupInspection | null>(null)
/** The backup the restore path picked. */
const backup = shallowRef<SetupBackup | null>(null)
/** The report survives the end of setup mode, for the cutover view. */
const report = shallowRef<SetupImportReport | null>(null)

const status = shallowRef<SetupStatus | null>(null)
/** An interrupted restore preselects the restore path once, so the person can still pick another. */
let restoreOffered = false
const statusError = shallowRef<unknown>(null)
async function refresh(): Promise<void> {
  try {
    status.value = await setupResource.status()
    statusError.value = null
    report.value = status.value.import?.report ?? report.value
    if (status.value.phase === 'import') path.value = 'import'
    else if (status.value.restore_incomplete && !restoreOffered) {
      restoreOffered = true
      path.value = 'restore'
    }
  } catch (error) {
    statusError.value = error
  }
}
onMounted(() => {
  if (runtimeConfig.setup) void refresh()
  else if (route.query.step === 'report') path.value = 'import'
})

const steps = computed<readonly SetupStep[]>(() => setupSteps[path.value])

/** In the import phase the server's state decides the step; before it, the address does. */
const step = computed<SetupStep | 'signin' | null>(() => {
  if (!runtimeConfig.setup) return path.value === 'import' || route.query.step === 'report' ? 'report' : 'site'
  if (!status.value) return null
  if (status.value.phase === 'import') {
    if (!status.value.account) return 'signin'
    const state = status.value.import?.state
    if (state === 'running') return 'import'
    if (state === 'complete') return 'report'
    return route.query.step === 'options' && inspection.value?.preview ? 'options' : 'source'
  }
  const requested = steps.value.find((candidate) => candidate === route.query.step)
  if (!token.value || !check.value || requested === undefined || requested === 'site') return 'token'
  return requested === 'confirm' && !backup.value ? 'backup' : requested
})

/** The address follows the server's state, so a reload after setup ends finds the cutover view. */
watch(step, (current) => {
  if (status.value?.phase === 'import' && current && current !== route.query.step) {
    void router.replace({ query: { step: current } })
  }
})

const position = computed(() => steps.value.indexOf(step.value === 'signin' ? 'admin' : (step.value ?? 'token')) + 1)

const go = (next: SetupStep) => router.push({ query: { step: next } })

const accepted = (value: string, result: SetupCheck) => {
  token.value = value
  check.value = result
  void go('check')
}

async function created(): Promise<void> {
  if (path.value === 'fresh') {
    await router.replace({ query: { step: 'site' } })
    return
  }
  await refresh()
  await router.replace({ query: { step: 'source' } })
}

const picked = (value: SetupBackup) => {
  backup.value = value
  void go('confirm')
}

/** The restored accounts sign in on the login page, which the server serves now. */
const restored = () => router.replace({ name: 'login' })

const inspected = (result: SetupInspection) => {
  inspection.value = result
  void go('options')
}

async function finished(): Promise<void> {
  await router.replace({ query: { step: 'report' } })
}

/** Continuing without an import ends setup like a fresh start: the first site comes next. */
const skippable = computed(() => step.value === 'source' || step.value === 'options' || step.value === 'report')
async function skipped(): Promise<void> {
  path.value = 'fresh'
  await router.replace({ query: { step: 'site' } })
}
</script>

<template>
  <div class="grid gap-7">
    <SetupStepBar
      v-if="step"
      :current="position"
      :total="steps.length"
    />
    <p
      v-if="statusError"
      class="text-sm text-muted"
      role="alert"
    >
      {{ t('setup.statusFailed') }}
    </p>
    <SetupTokenStep
      v-if="step === 'token'"
      @accepted="accepted"
    />
    <SetupCheckStep
      v-else-if="step === 'check' && check"
      :check="check"
      @continue="go('path')"
    />
    <SetupPathStep
      v-else-if="step === 'path'"
      v-model="path"
      :restore-incomplete="status?.restore_incomplete ?? false"
      @submit="go(steps[steps.indexOf('path') + 1] ?? 'path')"
    />
    <SetupAdminStep
      v-else-if="step === 'admin'"
      :token="token"
      :path="path"
      @created="created"
    />
    <SetupBackupStep
      v-else-if="step === 'backup'"
      :token="token"
      @picked="picked"
      @running="go('restore')"
    />
    <SetupConfirmStep
      v-else-if="step === 'confirm' && backup"
      :token="token"
      :backup="backup"
      @started="go('restore')"
      @back="go('backup')"
    />
    <SetupRestoreStep
      v-else-if="step === 'restore'"
      :token="token"
      @done="restored"
      @retry="go('backup')"
    />
    <SetupSignInStep
      v-else-if="step === 'signin'"
      @signed-in="refresh"
    />
    <SetupSourceStep
      v-else-if="step === 'source' && status?.import"
      v-model="source"
      :status="status.import"
      @inspected="inspected"
    />
    <SetupOptionsStep
      v-else-if="step === 'options' && status?.import && inspection?.preview"
      :source="source"
      :preview="inspection.preview"
      :status="status.import"
      @started="refresh"
    />
    <SetupImportStep
      v-else-if="step === 'import'"
      @settled="refresh"
    />
    <SetupReportStep
      v-else-if="step === 'report'"
      :report="report"
      @discarded="refresh"
      @finished="finished"
    />
    <SetupSiteStep v-else-if="step === 'site'" />
    <SetupSkipImport
      v-if="runtimeConfig.setup && skippable && status?.import"
      :imported="status.import.state !== 'none'"
      @skipped="skipped"
    />
  </div>
</template>
