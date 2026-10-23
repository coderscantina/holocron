<script setup lang="ts">
import { useMutation, useQuery } from '@tanstack/vue-query'

import { queryKeys } from '~/api/query-keys'
import { adminResource } from '~/api/resources/admin'
import { setupResource } from '~/api/resources/setup'
import FormField from '~/components/FormField.vue'
import Icon from '~/components/Icon.vue'
import { Alert } from '~/components/ui/alert'
import { Button } from '~/components/ui/button'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { useFormat } from '~/composables/useFormat'
import type { OperatorStatus, SetupImportReport } from '~/generated'
import CommandAlert from '~/pages/settings/CommandAlert.vue'
import { type MessageKey, useI18n } from '~/plugins/i18n'
import { runtimeConfig, serverStarted } from '~/services/runtime-config'

/**
 * The import report while setup mode waits for it to be acknowledged, then the cutover steps on
 * the running server. `report` is absent when the page was reloaded after setup ended.
 */
const props = defineProps<{ report: SetupImportReport | null }>()
const emit = defineEmits<{ discarded: []; finished: [] }>()

const { t } = useI18n()
const { message, fieldMessage } = useErrorMessage()
const { integer } = useFormat()

const outcomes: Record<string, MessageKey> = {
  kept: 'setup.report.outcomes.kept',
  reset_required: 'setup.report.outcomes.resetRequired',
  enrollment_required: 'setup.report.outcomes.enrollmentRequired',
  none: 'setup.report.outcomes.none',
}
const outcome = (code: string) => {
  const key = outcomes[code]
  return key ? t(key) : code
}

const sites = computed(() => props.report?.sites.filter((site) => site.kind === 'site') ?? [])
const totals = computed(() => {
  const all = props.report?.sites ?? []
  return {
    sites: integer(sites.value.length),
    sources: integer(all.length - sites.value.length),
    events: integer(all.reduce((sum, site) => sum + site.events, 0)),
  }
})
const notImported = computed(() => [
  ...(props.report?.events_not_imported ?? []),
  ...(props.report?.inventory ?? []),
])

const finish = useMutation({
  mutationFn: async () => {
    await setupResource.finish()
    await serverStarted()
  },
  onSuccess: () => emit('finished'),
})

const discarding = ref(false)
const confirm = ref('')
const discard = useMutation({
  mutationFn: () => setupResource.discard({ confirm: confirm.value }),
  onSuccess: () => emit('discarded'),
})
const discardError = computed(() => (discard.error.value instanceof Error ? discard.error.value : null))

/**
 * Accepted events since the counts started; the first hit after cutover raises it. Installation
 * wide on purpose: the install admin reads no analytics of imported sites, so a per-site marker
 * waits for Mike's decision on showing it per-site activity.
 */
const accepted = (status: OperatorStatus | undefined) =>
  status?.collection.outcomes
    .filter((count) => count.outcome === 'accepted')
    .reduce((sum, count) => sum + count.count, 0)
const operator = useQuery({
  queryKey: queryKeys.operatorStatus,
  queryFn: ({ signal }) => adminResource.operatorStatus(signal),
  enabled: computed(() => !runtimeConfig.setup),
  refetchInterval: 5000,
})
const baseline = ref<number | null>(null)
watch(
  () => accepted(operator.data.value),
  (count) => {
    if (count !== undefined && baseline.value === null) baseline.value = count
  }
)
const firstHit = computed(() => {
  const count = accepted(operator.data.value)
  return count !== undefined && baseline.value !== null && count > baseline.value
})

const scriptTag = `<script defer src="${window.location.origin}/script.js" data-website-id="…"></` + 'script>'
</script>

<template>
  <div
    v-if="runtimeConfig.setup && report"
    class="grid gap-7"
    data-wide
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ report.reconciled ? t('setup.report.reconciled') : t('setup.report.differences') }}
      <span class="block font-medium text-muted">{{ t('setup.report.totals', totals) }}</span>
    </h1>
    <Alert
      :color="report.reconciled ? 'success' : 'warning'"
    >
      <div class="grid gap-1.5 text-sm">
        <p>{{ report.reconciled ? t('setup.report.reconciledHint') : t('setup.report.differencesHint') }}</p>
        <ul
          v-if="report.differences.length > 0"
          class="grid gap-1 text-muted"
        >
          <li
            v-for="difference in report.differences"
            :key="difference"
            class="break-words"
          >
            {{ difference }}
          </li>
        </ul>
      </div>
    </Alert>

    <section
      v-if="report.logins.length > 0"
      class="grid gap-2"
    >
      <h2 class="text-sm font-semibold text-primary">{{ t('setup.report.logins') }}</h2>
      <div class="overflow-x-auto rounded-[var(--radius-control)] bg-fill">
        <table class="w-full text-left text-sm">
          <thead class="text-xs text-muted">
            <tr>
              <th class="px-4 pt-3 pb-1.5 font-medium">{{ t('setup.report.login') }}</th>
              <th class="px-4 pt-3 pb-1.5 font-medium">{{ t('auth.password') }}</th>
              <th class="px-4 pt-3 pb-1.5 font-medium">{{ t('setup.report.twoFactor') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="login in report.logins"
              :key="login.email"
              class="border-t border-hairline"
            >
              <td class="px-4 py-2 break-all text-primary">{{ login.email }}</td>
              <td class="px-4 py-2 text-muted">{{ outcome(login.password) }}</td>
              <td class="px-4 py-2 text-muted">{{ outcome(login.two_factor) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <section class="grid gap-2">
      <h2 class="text-sm font-semibold text-primary">{{ t('setup.report.notImported') }}</h2>
      <p
        v-if="notImported.length === 0"
        class="text-sm text-muted"
      >
        {{ t('setup.report.nothingLeft') }}
      </p>
      <details
        v-else
        class="rounded-[var(--radius-control)] bg-fill px-4 py-3 text-sm"
      >
        <summary class="cursor-pointer text-primary">
          {{ t('setup.report.notImportedHint', { count: integer(notImported.length) }) }}
        </summary>
        <dl class="mt-3 grid grid-cols-[1fr_auto] gap-x-6 gap-y-1">
          <template
            v-for="entry in notImported"
            :key="`${entry.table}:${entry.reason}`"
          >
            <dt class="min-w-0 truncate text-muted">
              <span class="font-mono text-xs">{{ entry.reason }}</span>
              <span class="text-xs"> · {{ entry.table || t('setup.report.events') }}</span>
            </dt>
            <dd class="text-right tabular-nums text-primary">{{ integer(entry.count) }}</dd>
          </template>
        </dl>
      </details>
    </section>

    <a
      :href="setupResource.manifestUrl()"
      download="holocron-import-manifest.json"
      class="flex w-fit items-center gap-2 text-sm font-medium text-accent-text hover:underline"
    >
      <Icon
        name="lucide:download"
        aria-hidden="true"
      />
      {{ t('setup.report.manifest') }}
    </a>

    <div class="grid gap-3.5">
      <Alert
        v-if="finish.error.value"
        color="destructive"
      >
        <p class="text-sm">{{ message(finish.error.value) }}</p>
      </Alert>
      <Button
        variant="accent"
        size="lg"
        :loading="finish.isPending.value"
        @click="finish.mutate()"
      >
        {{ finish.isPending.value ? t('setup.admin.starting') : t('setup.report.finish') }}
      </Button>
      <Button
        v-if="!discarding"
        variant="ghost"
        class="justify-self-start text-destructive"
        @click="discarding = true"
      >
        {{ t('setup.report.discard') }}
      </Button>
      <form
        v-else
        class="grid gap-3.5 rounded-[var(--radius-control)] bg-fill p-4"
        novalidate
        @submit.prevent="discard.mutate()"
      >
        <p class="text-sm text-muted">
          {{ t('setup.report.discardHint', { name: runtimeConfig.instance_name }) }}
        </p>
        <FormField
          id="setup-discard-confirm"
          v-model="confirm"
          autocomplete="off"
          :label="t('setup.report.discardLabel')"
          :error="fieldMessage(discardError, 'confirm')"
        />
        <CommandAlert
          :shown="['confirm']"
          :error="discardError"
        />
        <div class="flex gap-2">
          <Button
            type="submit"
            variant="destructive"
            :loading="discard.isPending.value"
            :disabled="confirm.trim() !== runtimeConfig.instance_name.trim()"
          >
            {{ t('setup.report.discardConfirm') }}
          </Button>
          <Button
            type="button"
            variant="ghost"
            @click="discarding = false"
          >
            {{ t('actions.cancel') }}
          </Button>
        </div>
      </form>
    </div>
  </div>

  <div
    v-else-if="!runtimeConfig.setup"
    class="grid gap-7"
    data-wide
  >
    <h1 class="text-4xl font-semibold tracking-tight text-primary">
      {{ t('setup.cutover.title') }}
      <span class="block font-medium text-muted">{{ t('setup.cutover.subtitle') }}</span>
    </h1>
    <div class="grid gap-1.5">
      <p class="text-sm font-medium text-primary">{{ t('setup.cutover.script') }}</p>
      <code class="block rounded-[var(--radius-control)] bg-fill p-3 font-mono text-xs break-all text-primary">
        {{ scriptTag }}
      </code>
    </div>
    <div class="grid gap-1.5 text-sm">
      <p class="text-muted">{{ t('setup.cutover.env') }}</p>
      <ul class="grid gap-1 font-mono text-xs text-primary">
        <li>TRACKER_SCRIPT_NAME → HOLOCRON_TRACKER_SCRIPT_NAME</li>
        <li>COLLECT_API_ENDPOINT → HOLOCRON_COLLECT_API_ENDPOINT</li>
      </ul>
    </div>
    <div
      class="flex items-center gap-3 rounded-[var(--radius-control)] bg-fill p-4 text-sm"
      role="status"
    >
      <span
        class="size-2 shrink-0 rounded-full"
        :class="firstHit ? 'bg-success' : 'animate-pulse bg-accent'"
        aria-hidden="true"
      />
      <span :class="firstHit ? 'text-primary' : 'text-muted'">
        {{ firstHit ? t('setup.cutover.firstHit') : t('setup.cutover.waiting') }}
      </span>
    </div>
    <p
      v-if="sites.length > 0"
      class="text-sm text-muted"
    >
      {{ t('setup.cutover.sites', { names: sites.map((site) => site.name || site.id).join(', ') }) }}
    </p>
    <Button
      variant="accent"
      size="lg"
      as-child
    >
      <RouterLink to="/">
        {{ t('setup.cutover.open') }}
      </RouterLink>
    </Button>
  </div>
</template>
