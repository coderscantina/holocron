<script setup lang="ts">
import Icon from '~/components/Icon.vue'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '~/components/ui/dropdown-menu'
import { buttonVariants } from '~/components/ui/button'
import { useSiteContext } from '~/composables/useSiteContext'
import { useSiteSummaries } from '~/composables/useSiteSummaries'
import TeamCreateDialog from '~/components/workspace/TeamCreateDialog.vue'
import { groupByBaseDomain } from '~/lib/base-domain'
import { kindLabels, roleLabels } from '~/lib/roles'
import { findingCount, signedPercent, sitePace, summaryTime, summaryWeekday, type SitePace } from '~/lib/site-pace'
import type { SiteId } from '~/generated'
import { useI18n } from '~/plugins/i18n'

/**
 * Workspace / site context chips, left to right. Options come from
 * `workspaces.list`, never from a client-side guess. No environment control: the product
 * defines no environment setting (decision recorded in the extraction map). Pace comes from the
 * site summaries only; a stale or missing summary shows the name alone.
 */
const { t, locale } = useI18n()
const context = useSiteContext()

const creatingTeam = ref(false)

const siteGroups = computed(() => groupByBaseDomain(context.workspace.value?.sites ?? []))

const initial = (name: string) => name.trim().charAt(0).toUpperCase() || '?'

const onWorkspace = (id: unknown) => {
  const next = context.workspaces.value.find((workspace) => workspace.id === id)
  if (next && next.id !== context.workspace.value?.id) void context.selectWorkspace(next)
}

const onSite = (id: unknown) => {
  const next = context.workspace.value?.sites.find((site) => site.id === id)
  if (next && next.id !== context.site.value?.id) void context.selectSite(next)
}

const siteIds = computed(() => context.workspace.value?.sites.map((site) => site.id) ?? [])
const { summaries, refreshIfStale } = useSiteSummaries(
  () => context.workspace.value?.id ?? null,
  siteIds,
  () => context.site.value?.id ?? null
)

/**
 * Findings per site that has a fresh summary with at least one. The dot is always the accent:
 * the design makes it destructive when any finding is a decrease, but `Summary` carries only
 * the count, not the direction.
 */
const findingCounts = computed(
  () =>
    new Map(
      siteIds.value.flatMap((id) => {
        const count = findingCount(summaries.value.get(id))
        return count > 0 ? [[id, count] as const] : []
      })
    )
)

const paceOf = (id: SiteId) => sitePace(summaries.value.get(id))

const paceText = (pace: SitePace) => {
  switch (pace.kind) {
    case 'new':
      return t('context.pace.new')
    case 'quiet':
      return t('context.pace.quiet', { count: pace.views })
    case 'usual':
      return t('context.pace.usual')
    default:
      return t('context.pace.change', { change: signedPercent(pace.ratio) })
  }
}

const menuPaces = computed(
  () =>
    new Map(
      siteIds.value.flatMap((id) => {
        const pace = paceOf(id)
        return pace ? [[id, paceText(pace)] as const] : []
      })
    )
)

/** The chip shows a number only where the sentence behind it is "above", "below" or "usual". */
const chipPace = computed(() => {
  const id = context.site.value?.id
  const summary = id ? summaries.value.get(id) : undefined
  const pace = id ? paceOf(id) : null
  if (!summary || !pace || (pace.kind !== 'above' && pace.kind !== 'below' && pace.kind !== 'usual')) return null
  const percent = Math.abs(Math.round(pace.ratio * 100))
  const weekday = summaryWeekday(summary, locale.value)
  return {
    text: `${signedPercent(pace.ratio)}\u00a0%`,
    tone: pace.kind === 'above' ? 'text-success' : pace.kind === 'below' ? 'text-destructive' : 'text-muted',
    label: t(`context.pace.${pace.kind === 'usual' ? 'usualLabel' : pace.kind === 'above' ? 'aboveLabel' : 'belowLabel'}`, { percent, weekday }),
  }
})

/** The chip's name is its visible text, site and pace figure; the pace in words is the description. */
const paceId = useId()

const footer = computed(() => {
  const fresh = siteIds.value.flatMap((id) => {
    const summary = summaries.value.get(id)
    return summary && !summary.stale ? [summary] : []
  })
  if (fresh.length === 0) return null
  const latest = Math.max(...fresh.map((summary) => summary.computed_at))
  return {
    time: t('context.pace.from', { time: summaryTime(latest, locale.value) }),
    behind: fresh.length < siteIds.value.length,
  }
})

const triggerClass = buttonVariants({ variant: 'fill', size: 'control', class: 'shrink min-w-0 font-medium' })
</script>

<template>
  <div class="flex min-w-0 items-center gap-2">
    <DropdownMenu>
      <DropdownMenuTrigger
        :class="triggerClass"
        :aria-label="
          context.workspace.value
            ? t('context.workspaceTrigger', { name: `${context.workspace.value.name} ${t(kindLabels[context.workspace.value.kind])}` })
            : t('context.chooseWorkspace')
        "
      >
        <span
          class="grid size-5 shrink-0 place-items-center rounded-md bg-accent text-2xs font-bold text-accent-foreground before:content-[attr(data-initial)]"
          :data-initial="context.workspace.value ? initial(context.workspace.value.name) : '?'"
          aria-hidden="true"
        />
        <span class="hidden max-w-28 truncate @lg:inline @3xl:max-w-48">
          {{ context.workspace.value?.name ?? t('context.chooseWorkspace') }}
        </span>
        {{ ' ' }}
        <span
          v-if="context.workspace.value"
          class="hidden shrink-0 rounded-sm bg-fill-strong px-1.5 text-2xs font-medium text-muted @3xl:inline"
        >
          {{ t(kindLabels[context.workspace.value.kind]) }}
        </span>
        <Icon
          name="lucide:chevron-down"
          size="14"
          class="shrink-0 text-muted"
          aria-hidden="true"
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="start"
        class="w-72"
      >
        <DropdownMenuLabel class="text-xs font-medium text-muted">{{ t('context.workspaces') }}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          :model-value="context.workspace.value?.id ?? ''"
          @update:model-value="onWorkspace"
        >
          <DropdownMenuRadioItem
            v-for="workspace in context.workspaces.value"
            :key="workspace.id"
            :value="workspace.id"
            class="gap-2"
          >
            <span class="min-w-0 flex-1 truncate">{{ workspace.name }}</span>
            <span class="shrink-0 text-xs text-muted in-data-highlighted:text-accent-foreground">{{ t(roleLabels[workspace.role]) }}</span>
          </DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuItem
          v-if="context.workspaces.value.length === 0"
          disabled
        >
          {{ t('context.noWorkspaces') }}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          v-if="context.workspace.value"
          as-child
        >
          <RouterLink :to="{ name: 'workspace', params: { workspaceId: context.workspace.value.id } }">
            <Icon name="lucide:building-2" />
            {{ t('workspace.manage') }}
          </RouterLink>
        </DropdownMenuItem>
        <DropdownMenuItem @select="creatingTeam = true">
          <Icon name="lucide:plus" />
          {{ t('workspace.createTeam') }}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    <TeamCreateDialog v-model:open="creatingTeam" />

    <template v-if="context.workspace.value">
      <DropdownMenu @update:open="(open) => open && refreshIfStale()">
        <DropdownMenuTrigger
          :class="triggerClass"
          :aria-label="
            context.site.value
              ? t('context.siteTrigger', { name: chipPace ? `${context.site.value.name} ${chipPace.text}` : context.site.value.name })
              : t('context.chooseSite')
          "
          :aria-describedby="chipPace ? paceId : undefined"
        >
          <span class="max-w-32 truncate @md:max-w-48 @3xl:max-w-72">
            {{ context.site.value?.name ?? t('context.chooseSite') }}
          </span>
          <span
            v-if="chipPace"
            class="shrink-0 text-xs font-medium tabular-nums"
            :class="chipPace.tone"
            aria-hidden="true"
          >
            {{ chipPace.text }}
          </span>
          <span
            v-if="chipPace"
            :id="paceId"
            class="sr-only"
          >
            {{ chipPace.label }}
          </span>
          <Icon
            name="lucide:chevron-down"
            size="14"
            class="shrink-0 text-muted"
            aria-hidden="true"
          />
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="start"
          class="max-h-[min(30rem,var(--reka-dropdown-menu-content-available-height))] w-80 max-w-[calc(100vw-2rem)] overflow-y-auto"
        >
          <DropdownMenuItem
            as-child
            class="gap-2.5"
          >
            <RouterLink :to="{ name: 'workspace', params: { workspaceId: context.workspace.value.id } }">
              <Icon
                name="lucide:layout-list"
                class="text-muted"
                aria-hidden="true"
              />
              <span class="grid min-w-0">
                <span class="truncate font-medium">{{ t('context.allSites', { workspace: context.workspace.value.name }) }}</span>
                <span class="truncate text-xs text-muted in-data-highlighted:text-accent-foreground">
                  {{ t('context.siteCount', { count: context.workspace.value.sites.length }) }}
                </span>
              </span>
            </RouterLink>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuRadioGroup
            :model-value="context.site.value?.id ?? ''"
            @update:model-value="onSite"
          >
            <template
              v-for="group in siteGroups"
              :key="group.baseDomain"
            >
              <DropdownMenuLabel
                v-if="siteGroups.length > 1"
                class="truncate px-2 pt-2 pb-1 text-xs font-medium text-muted"
              >
                {{ group.baseDomain }}
              </DropdownMenuLabel>
              <DropdownMenuRadioItem
                v-for="site in group.sites"
                :key="site.id"
                :value="site.id"
              >
                <span class="grid min-w-0 flex-1">
                  <span class="truncate">{{ site.name }}</span>
                  <span class="truncate text-xs text-muted in-data-highlighted:text-accent-foreground">{{ site.domain }}</span>
                  <span
                    v-if="menuPaces.get(site.id)"
                    class="truncate text-xs text-muted in-data-highlighted:text-accent-foreground"
                  >
                    {{ menuPaces.get(site.id) }}
                  </span>
                </span>
                <template v-if="findingCounts.get(site.id)">
                  <span
                    class="size-2 shrink-0 rounded-full bg-accent"
                    aria-hidden="true"
                  />
                  <span class="sr-only">{{ t('context.findings', { count: findingCounts.get(site.id) }) }}</span>
                </template>
              </DropdownMenuRadioItem>
            </template>
          </DropdownMenuRadioGroup>
          <DropdownMenuItem
            v-if="context.workspace.value.sites.length === 0"
            disabled
          >
            {{ t('context.noSites') }}
          </DropdownMenuItem>
          <template v-if="footer">
            <DropdownMenuSeparator />
            <p class="grid px-2 py-1.5 text-xs text-muted">
              <span>{{ footer.time }}</span>
              <span v-if="footer.behind">{{ t('context.pace.behind') }}</span>
            </p>
          </template>
        </DropdownMenuContent>
      </DropdownMenu>
    </template>
  </div>
</template>
