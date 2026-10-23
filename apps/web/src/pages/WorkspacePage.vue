<script setup lang="ts">
import PageHeader from '~/components/PageHeader.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { segmentedItem, segmentedTrack } from '~/components/ui/segmented'
import { Skeleton } from '~/components/ui/skeleton'
import { useErrorMessage } from '~/composables/useErrorMessage'
import { isRefusal } from '~/composables/useExplore'
import { usePageMeta } from '~/composables/usePageMeta'
import { useTeam } from '~/composables/useTeams'
import { kindLabels, roleLabels } from '~/lib/roles'
import { useI18n, type MessageKey } from '~/plugins/i18n'

/**
 * One workspace: its sites, links and pixels, members and settings. What the caller may change comes from the
 * team's capabilities; a workspace the caller cannot see shows the same state as a missing one.
 */
const { t } = useI18n()
const { message } = useErrorMessage()
const route = useRoute()

const workspaceId = computed(() => String(route.params.workspaceId))
const { query, can } = useTeam(workspaceId)
const team = computed(() => query.data.value?.team ?? null)

usePageMeta(() => team.value?.name ?? t('context.workspaces'))

/** Short title-row context: the workspace kind and the caller's role in it. */
const context = computed(() =>
  team.value ? [t(kindLabels[team.value.kind]), team.value.role ? t(roleLabels[team.value.role]) : null].filter(Boolean).join(' · ') : undefined
)

const sections = computed(() => {
  const list: { name: string; labelKey: MessageKey }[] = [
    { name: 'workspace', labelKey: 'workspace.sites.title' },
    { name: 'workspace-links', labelKey: 'links.title' },
  ]
  if (team.value?.kind === 'team') list.push({ name: 'workspace-members', labelKey: 'workspace.members' })
  if (team.value?.kind === 'team' && can('team.manage')) list.push({ name: 'workspace-settings', labelKey: 'workspace.settings' })
  return list
})
</script>

<template>
  <div
    v-if="query.isPending.value"
    class="grid gap-4"
    aria-busy="true"
    :aria-label="t('states.loading')"
  >
    <Skeleton class="h-10 w-64" />
    <Skeleton class="h-48 w-full" />
  </div>
  <EmptyState
    v-else-if="isRefusal(query.error.value)"
    icon="lucide:shield-question"
    :title="t('workspace.unavailable')"
    class="mx-auto max-w-lg"
  >
    <Button
      as-child
      variant="outline"
    >
      <RouterLink :to="{ name: 'home' }">{{ t('site.unavailable.home') }}</RouterLink>
    </Button>
  </EmptyState>
  <EmptyState
    v-else-if="query.isError.value"
    icon="lucide:triangle-alert"
    tone="error"
    :title="t('workspace.loadFailed')"
    :description="message(query.error.value)"
    class="mx-auto max-w-lg"
  >
    <Button
      variant="outline"
      @click="query.refetch()"
    >
      {{ t('actions.retry') }}
    </Button>
  </EmptyState>
  <template v-else-if="team && query.data.value">
    <PageHeader
      :title="team.name"
      :description="context"
    >
      <template
        v-if="sections.length > 1"
        #actions
      >
        <nav
          :aria-label="t('workspace.sections')"
          :class="segmentedTrack"
        >
          <RouterLink
            v-for="section in sections"
            :key="section.name"
            :to="{ name: section.name, params: { workspaceId } }"
            :class="segmentedItem({ size: 'md' })"
            :data-state="route.name === section.name ? 'active' : undefined"
          >
            {{ t(section.labelKey) }}
          </RouterLink>
        </nav>
      </template>
    </PageHeader>
    <RouterView v-slot="{ Component }">
      <component
        :is="Component"
        :key="team.id"
        :detail="query.data.value"
        :revision="query.data.value.revision"
      />
    </RouterView>
  </template>
</template>
