import type {
  AnnotationsCreateArgs,
  AnnotationsUpdateArgs,
  ExplorationState,
  ExportFormat,
  Field,
  Group,
  Panel,
  SiteId,
} from '~/generated'

import { api } from '..'
import type { ApiClient } from '../client'

/**
 * Explorations of one site. States travel as their JSON text in one argument, the same
 * document a URL, saved exploration and board card carry. `explore` and `export` take another
 * client for share viewers, who read through the share's credential.
 */
export const explorationsResource = {
  explore: (state: ExplorationState, panels: Panel[], signal?: AbortSignal, client: ApiClient = api) =>
    client.query('analytics.explore', { state: JSON.stringify(state), panels: panels.map((p) => JSON.stringify(p)) }, { signal }),
  export: (state: ExplorationState, format: ExportFormat, client: ApiClient = api) =>
    client.query('analytics.export', { state: JSON.stringify(state), format }),
  catalog: (site: SiteId, signal?: AbortSignal) => api.query('fields.catalog', { site }, { signal }),
  values: (site: SiteId, field: Field, signal?: AbortSignal) =>
    api.query('fields.values', { site, field: JSON.stringify(field) }, { signal }),
  realtime: (site: SiteId, signal?: AbortSignal) => api.query('realtime.snapshot', { site }, { signal }),
  session: (site: SiteId, session: number, signal?: AbortSignal) =>
    api.query('sessions.get', { site, session }, { signal }),
  /** The latest sessions starting in the last `hours`, with their events, for the timeline. */
  sessionTimeline: (site: SiteId, hours: number, signal?: AbortSignal) =>
    api.query('sessions.timeline', { site, hours }, { signal }),
  saved: (site: SiteId, signal?: AbortSignal) => api.query('explorations.list', { site }, { signal }),
  save: (site: SiteId, name: string, state: ExplorationState, exploration?: string) =>
    api.command('explorations.save', { site, name, state, exploration }),
  remove: (site: SiteId, exploration: string) => api.command('explorations.delete', { site, exploration }),
  segments: (site: SiteId, signal?: AbortSignal) => api.query('segments.list', { site }, { signal }),
  saveSegment: (site: SiteId, name: string, filter: Group) => api.command('segments.save', { site, name, filter }),
  removeSegment: (site: SiteId, segment: string) => api.command('segments.delete', { site, segment }),
  annotations: (site: SiteId, start: number, end: number, signal?: AbortSignal) =>
    api.query('annotations.list', { site, start, end }, { signal }),
  annotate: (args: AnnotationsCreateArgs) => api.command('annotations.create', args),
  updateAnnotation: (args: AnnotationsUpdateArgs) => api.command('annotations.update', args),
  removeAnnotation: (site: SiteId, annotation: string) => api.command('annotations.delete', { site, annotation }),
}
