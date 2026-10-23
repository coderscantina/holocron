<script setup lang="ts">
import { useDocumentVisibility, useFullscreen, useIntervalFn, useNow } from '@vueuse/core'

import ChoiceGroup from '~/components/report/ChoiceGroup.vue'
import SiteCreateDialog from '~/components/workspace/SiteCreateDialog.vue'
import Icon from '~/components/Icon.vue'
import { Button } from '~/components/ui/button'
import { EmptyState } from '~/components/ui/empty-state'
import { Panel } from '~/components/ui/panel'
import { SimpleTooltip } from '~/components/ui/tooltip'
import { useSiteContext } from '~/composables/useSiteContext'
import { useSiteSummaries } from '~/composables/useSiteSummaries'
import { useViewMode } from '~/composables/useViewMode'
import type { TeamDetail } from '~/generated'
import { useI18n } from '~/plugins/i18n'

import SiteStatusRow from './SiteStatusRow.vue'
import SiteTile from './SiteTile.vue'
import { arrangeSites, needingAttention, sortSites } from './site-status'

/**
 * How the workspace's sites are doing today, from their summaries (one request, no per-site
 * query), and a new site for callers who may write to the workspace. Two views, remembered in
 * this browser: tiles (default), which also run full screen as a wall display, and the
 * details list.
 */
const props = defineProps<{ detail: TeamDetail; revision: number }>()

const { t, locale } = useI18n()
const context = useSiteContext()
const creating = ref(false)

const sites = computed(() => context.workspaces.value.find((entry) => entry.id === props.detail.team.id)?.sites ?? [])
const canCreate = computed(() => props.detail.team.capabilities.includes('workspace.write'))

const { query, summaries, refreshIfStale } = useSiteSummaries(
  () => props.detail.team.id,
  () => sites.value.map((site) => site.id)
)
const groups = computed(() => arrangeSites(sites.value, summaries.value))
const ordered = computed(() => sortSites(sites.value, summaries.value))
const attention = computed(() => needingAttention(sites.value, summaries.value))

const view = useViewMode('workspace-sites', ['tiles', 'details'])
const views = computed(() => [
  { value: 'tiles' as const, label: t('workspace.sites.view.tiles') },
  { value: 'details' as const, label: t('workspace.sites.view.details') },
])

const root = useTemplateRef('root')
const screen = useFullscreen(root)
const wall = computed(() => screen.isFullscreen.value)

/** Tiles are left open on a wall: read the summaries again once a minute while they show. */
const visibility = useDocumentVisibility()
useIntervalFn(() => {
  if (view.value === 'tiles' && visibility.value === 'visible') refreshIfStale()
}, 60_000)

const now = useNow({ interval: 15_000 })
const clock = computed(() => new Intl.DateTimeFormat(locale.value, { timeStyle: 'short' }).format(now.value))
</script>

<template>
  <div
    ref="root"
    class="grid content-start gap-4"
    :class="wall ? 'h-svh grid-rows-[auto_minmax(0,1fr)] bg-surface p-shell-page-x' : ''"
  >
    <EmptyState
      v-if="sites.length === 0"
      icon="lucide:globe"
      :title="t('workspace.noSites')"
      :description="canCreate ? t('workspace.noSitesHint') : undefined"
      class="mx-auto w-full max-w-lg"
    >
      <Button
        v-if="canCreate"
        variant="accent"
        @click="creating = true"
      >
        <Icon name="lucide:plus" />
        {{ t('workspace.addSite') }}
      </Button>
    </EmptyState>
    <template v-else>
      <div
        class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 text-muted"
        :class="wall ? 'text-lg' : 'text-sm'"
      >
        <p class="flex flex-wrap items-baseline gap-x-3">
          <span
            v-if="wall"
            class="text-2xl font-semibold text-primary"
          >
            {{ detail.team.name }}
          </span>
          <span>
            {{ t('context.siteCount', { count: sites.length }) }},
            {{ attention > 0 ? t('workspace.sites.needYou', { count: attention }) : t('workspace.sites.nothingNeedsYou') }}
          </span>
        </p>
        <div class="flex flex-wrap items-center gap-3">
          <p
            v-if="query.isError.value"
            role="status"
          >
            {{ t('workspace.sites.statusFailed') }}
          </p>
          <span
            v-if="wall"
            class="text-2xl font-semibold text-primary tabular-nums"
          >
            {{ clock }}
          </span>
          <ChoiceGroup
            v-else
            v-model="view"
            :label="t('workspace.sites.view.label')"
            :options="views"
          />
          <SimpleTooltip
            v-if="view === 'tiles' && screen.isSupported.value"
            :tooltip="wall ? t('stage.exitFullscreen') : t('stage.fullscreen')"
            as-child
          >
            <Button
              variant="ghost"
              size="icon"
              class="text-muted hover:text-primary"
              :aria-label="wall ? t('stage.exitFullscreen') : t('stage.fullscreen')"
              @click="screen.toggle()"
            >
              <Icon
                :name="wall ? 'lucide:minimize-2' : 'lucide:maximize-2'"
                aria-hidden="true"
              />
            </Button>
          </SimpleTooltip>
        </div>
      </div>

      <ul
        v-if="view === 'tiles'"
        class="grid gap-4"
        :class="wall ? 'auto-rows-fr grid-cols-[repeat(auto-fit,minmax(min(100%,24rem),1fr))]' : 'grid-cols-[repeat(auto-fill,minmax(min(100%,17rem),1fr))]'"
        :aria-label="t('workspace.sites.listLabel')"
      >
        <li
          v-for="site in ordered"
          :key="site.id"
          class="min-w-0"
        >
          <SiteTile
            :site="site"
            :summary="summaries.get(site.id)"
            :loading="query.isPending.value"
            :large="wall"
          />
        </li>
      </ul>
      <div
        v-else
        class="grid gap-4"
      >
        <Panel
          v-for="group in groups"
          :key="group.baseDomain"
          :title="group.baseDomain || undefined"
          :meta="group.baseDomain ? t('context.siteCount', { count: group.sites.length }) : undefined"
          :aria-label="group.baseDomain || t('workspace.sites.listLabel')"
          flush
        >
          <ul class="divide-y divide-divider">
            <li
              v-for="site in group.sites"
              :key="site.id"
            >
              <SiteStatusRow
                :site="site"
                :summary="summaries.get(site.id)"
                :loading="query.isPending.value"
              />
            </li>
          </ul>
        </Panel>
      </div>
      <div v-if="canCreate && !wall">
        <Button
          variant="fill"
          class="text-sm"
          @click="creating = true"
        >
          <Icon name="lucide:plus" />
          {{ t('workspace.addSite') }}
        </Button>
      </div>
    </template>
    <SiteCreateDialog
      v-model:open="creating"
      :workspace="detail.team.id"
    />
  </div>
</template>
