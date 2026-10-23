import type {
  CaptureArgs,
  HeatmapCellsArgs,
  HeatmapPagesArgs,
  RecordingPolicyUpdateArgs,
  RecordingsGetArgs,
  RecordingsListArgs,
  SiteId,
} from '~/generated'

import { api } from '..'
import type { ApiClient } from '../client'

export const recordingsResource = {
  policy: (site: SiteId, signal?: AbortSignal) => api.query('recordings.policy', { site }, { signal }),
  updatePolicy: (args: RecordingPolicyUpdateArgs, idempotencyKey?: string) =>
    api.command('recordings.updatePolicy', args, { idempotencyKey }),
  budget: (site: SiteId, signal?: AbortSignal) => api.query('recordings.budget', { site }, { signal }),
  /** `client` reads as a share viewer whose share grants recordings. */
  list: (args: RecordingsListArgs, signal?: AbortSignal, client: ApiClient = api) =>
    client.query('recordings.list', args, { signal }),
  /** One playback page. Its `events` are untrusted and go to the replay frame only. */
  get: (args: RecordingsGetArgs, signal?: AbortSignal, client: ApiClient = api) =>
    client.query('recordings.get', args, { signal }),
  save: (args: CaptureArgs) => api.command('recordings.save', args),
  unsave: (args: CaptureArgs) => api.command('recordings.unsave', args),
  heatmapPages: (args: HeatmapPagesArgs, signal?: AbortSignal) => api.query('heatmaps.pages', args, { signal }),
  heatmapCells: (args: HeatmapCellsArgs, signal?: AbortSignal) => api.query('heatmaps.cells', args, { signal }),
}
