import { createSharedComposable } from '@vueuse/core'
import { computed } from 'vue'
import type { RouteLocationRaw } from 'vue-router'

import { explorationsResource } from '~/api/resources/explorations'
import { useCurrentFindings } from '~/composables/useCurrentFindings'
import { useLiveSnapshot } from '~/composables/useLive'
import { useSiteContext } from '~/composables/useSiteContext'
import type { Finding } from '~/generated'
import { workspaceAreas } from '~/lib/navigation'
import type { MessageKey } from '~/plugins/i18n'

/**
 * What a row says beyond its label. `count` is an attention badge ("a finding here needs you");
 * `label` is a message taking `{count}`, which names the badge for screen readers and tooltips.
 * `live` is the current site's active visitor count.
 */
export type NavigationIndicator =
  | { kind: 'count'; value: number; label: MessageKey }
  | { kind: 'live'; value: number }

export interface NavigationItem {
  id: string
  labelKey: MessageKey
  icon: string
  to: RouteLocationRaw
  indicator?: NavigationIndicator
}

export interface NavigationSection {
  id: string
  heading: MessageKey
  items: NavigationItem[]
}

/** The groups of the site section, in sidebar order, as route names. */
const siteGroups = [
  { id: 'traffic', heading: 'nav.sections.traffic', items: ['overview', 'explore', 'realtime', 'sessions'] },
  { id: 'behaviour', heading: 'nav.sections.behaviour', items: ['events', 'funnels', 'goals', 'journeys', 'retention'] },
  { id: 'acquisition', heading: 'nav.sections.acquisition', items: ['campaigns', 'attribution', 'revenue'] },
  { id: 'experience', heading: 'nav.sections.experience', items: ['performance', 'replays', 'heatmaps'] },
] as const satisfies readonly { id: string; heading: MessageKey; items: readonly string[] }[]

/**
 * The navigation item each finding family badges. Dimension movers badge nothing: they live
 * on the overview, which is already the page everything starts from.
 */
const findingItems = {
  goal_pace: 'goals',
  funnel_step: 'funnels',
  performance: 'performance',
} as const satisfies Partial<Record<Finding['values']['kind'], string>>

/** Findings per navigation item, counting only the families that have an item. */
export function findingBadges(findings: readonly Finding[]): Map<string, number> {
  const counts = new Map<string, number>()
  for (const { values } of findings) {
    if (values.kind === 'dimension_mover') continue
    const id = findingItems[values.kind]
    counts.set(id, (counts.get(id) ?? 0) + 1)
  }
  return counts
}

/**
 * Active visitors of the current site, read from the realtime snapshot and its live
 * subscription. One instance serves every sidebar and drawer; null whenever the subscription
 * is not current, so a stale figure never passes for a live one.
 */
const useLiveCount = createSharedComposable(() => {
  const context = useSiteContext()
  const live = useLiveSnapshot(
    () => context.site.value?.id ?? null,
    (site, signal) => explorationsResource.realtime(site, signal)
  )
  return computed(() => (live.status.value === 'current' ? (live.data.value?.active ?? null) : null))
})

/**
 * Contextual analytics navigation: workspace areas, then the current site's reports. Context
 * selection and the account live in the header, never here.
 */
export function useNavigation() {
  const context = useSiteContext()
  const liveCount = useLiveCount()
  const siteFindings = useCurrentFindings()

  /** Stale findings are not shown, as on the overview. */
  const badges = computed(() => (siteFindings.stale.value ? new Map<string, number>() : findingBadges(siteFindings.findings.value)))

  const workspaceItems = computed<NavigationItem[]>(() =>
    Object.entries(workspaceAreas).map(([id, area]) => ({
      id,
      labelKey: area.labelKey,
      icon: area.icon,
      to: { name: id },
    }))
  )

  const siteItems = computed<NavigationItem[]>(() => {
    const siteId = context.site.value?.id
    if (!siteId) return []
    const live = liveCount.value
    const items: NavigationItem[] = [
      { id: 'overview', labelKey: 'nav.overview', icon: 'lucide:chart-column', to: { name: 'overview', params: { siteId } } },
      { id: 'explore', labelKey: 'nav.explore', icon: 'lucide:search', to: { name: 'explore', params: { siteId } } },
      { id: 'realtime', labelKey: 'nav.realtime', icon: 'lucide:activity', to: { name: 'realtime', params: { siteId } }, indicator: live === null ? undefined : { kind: 'live', value: live } },
      { id: 'events', labelKey: 'nav.events', icon: 'lucide:zap', to: { name: 'events', params: { siteId } } },
      { id: 'sessions', labelKey: 'nav.sessions', icon: 'lucide:users', to: { name: 'sessions', params: { siteId } } },
      { id: 'funnels', labelKey: 'nav.funnels', icon: 'lucide:filter', to: { name: 'funnels', params: { siteId } } },
      { id: 'goals', labelKey: 'nav.goals', icon: 'lucide:flag', to: { name: 'goals', params: { siteId } } },
      { id: 'journeys', labelKey: 'nav.journeys', icon: 'lucide:route', to: { name: 'journeys', params: { siteId } } },
      { id: 'campaigns', labelKey: 'nav.campaigns', icon: 'lucide:megaphone', to: { name: 'campaigns', params: { siteId } } },
      { id: 'attribution', labelKey: 'nav.attribution', icon: 'lucide:git-merge', to: { name: 'attribution', params: { siteId } } },
      { id: 'retention', labelKey: 'nav.retention', icon: 'lucide:target', to: { name: 'retention', params: { siteId } } },
      { id: 'revenue', labelKey: 'nav.revenue', icon: 'lucide:euro', to: { name: 'revenue', params: { siteId } } },
      { id: 'performance', labelKey: 'nav.performance', icon: 'lucide:gauge', to: { name: 'performance', params: { siteId } } },
      { id: 'replays', labelKey: 'nav.replays', icon: 'lucide:circle-play', to: { name: 'replays', params: { siteId } } },
      { id: 'heatmaps', labelKey: 'nav.heatmaps', icon: 'lucide:mouse-pointer-click', to: { name: 'heatmaps', params: { siteId } } },
    ]
    return items.map((item): NavigationItem => {
      const value = badges.value.get(item.id)
      return value ? { ...item, indicator: { kind: 'count', value, label: 'nav.indicator.findings' } } : item
    })
  })

  const sections = computed<NavigationSection[]>(() => {
    const list: NavigationSection[] = [
      { id: 'workspace', heading: 'nav.sections.workspace', items: workspaceItems.value },
    ]
    const byId = new Map(siteItems.value.map((item) => [item.id, item]))
    for (const group of siteGroups) {
      const items = group.items.flatMap((id) => byId.get(id) ?? [])
      if (items.length > 0) list.push({ id: group.id, heading: group.heading, items })
    }
    return list
  })

  /** Site settings sit apart at the bottom of the sidebar. */
  const settings = computed<NavigationItem | null>(() => {
    const siteId = context.site.value?.id
    return siteId
      ? { id: 'settings', labelKey: 'nav.siteSettings', icon: 'lucide:settings', to: { name: 'site-settings', params: { siteId } } }
      : null
  })

  /** Thumb-reach tabs on a phone: overview, explore, realtime. */
  const tabs = computed<NavigationItem[]>(() => {
    const byId = new Map([...siteItems.value, ...workspaceItems.value].map((item) => [item.id, item]))
    return ['overview', 'explore', 'realtime'].flatMap((id) => byId.get(id) ?? [])
  })

  return { sections, settings, tabs, items: computed(() => [...workspaceItems.value, ...siteItems.value]) }
}
