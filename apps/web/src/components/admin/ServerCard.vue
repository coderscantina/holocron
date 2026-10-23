<script setup lang="ts">
import { Progress } from '~/components/ui/progress'
import { useOperatorStatus } from '~/composables/useOperatorStatus'
import { blocksCollection, share, useOperatorStatusText, usedBytes } from '~/lib/operator-status'
import { useI18n } from '~/plugins/i18n'

/**
 * The sidebar server card (design contract Server card): the "This server" heading, the state in words, a thin disk
 * meter and the storage forecast sentence, linking to the admin status page.
 *
 * Usage: `<ServerCard />` with no props, at the bottom of the sidebar above Site settings. It
 * reads `operator.status` itself and renders nothing at all for anyone who is not an install
 * admin, or until the first answer arrives, so the sidebar reserves no space for it.
 */
const { t } = useI18n()
const text = useOperatorStatusText()
const { isInstallAdmin, query } = useOperatorStatus()

const status = computed(() => (isInstallAdmin.value ? query.data.value : undefined))
const tone = computed(() => {
  const problems = status.value?.problems ?? []
  if (problems.some(blocksCollection)) return 'destructive'
  return problems.length > 0 ? 'warning' : 'success'
})
const disk = computed(() => {
  if (!status.value) return null
  const used = usedBytes(status.value.disk)
  const free = status.value.disk.free_bytes
  return { used, free, percent: share(used, used + free) }
})
</script>

<template>
  <section
    v-if="status && disk"
    :aria-label="t('admin.status.card.label')"
    class="relative grid gap-2 rounded-lg bg-fill p-3 text-sm focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring"
  >
    <p class="text-xs text-muted">{{ t('admin.status.card.label') }}</p>
    <p class="flex items-center gap-2">
      <span
        class="size-2 shrink-0 rounded-full"
        :class="{ 'bg-success': tone === 'success', 'bg-warning': tone === 'warning', 'bg-destructive': tone === 'destructive' }"
        aria-hidden="true"
      />
      <RouterLink
        :to="{ name: 'admin-status' }"
        class="truncate font-medium text-primary outline-none after:absolute after:inset-0"
        :title="t('admin.status.card.open')"
      >
        {{ text.state(status.problems) }}
      </RouterLink>
    </p>
    <Progress
      class="h-1 bg-fill-strong"
      :model-value="disk.percent"
      :variant="tone === 'destructive' ? 'destructive' : 'default'"
      :aria-label="t('admin.status.budgets.diskUsed', { used: text.bytes(disk.used), free: text.bytes(disk.free) })"
    />
    <p class="text-xs text-muted">{{ text.forecast(status.forecast) }}</p>
  </section>
</template>
