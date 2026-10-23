import type {
  ExplorationState,
  Link,
  LinkId,
  LinksCreateArgs,
  LinksUpdateArgs,
  Panel,
  Pixel,
  PixelId,
  PixelsCreateArgs,
  PixelsUpdateArgs,
  WorkspaceId,
} from '~/generated'

import { api } from '..'

/** A link (`/q/<slug>`) or a pixel (`/p/<slug>`) of a workspace (decision 0020). */
export type SourceKind = 'link' | 'pixel'

/** Links are the sources with a target URL. */
export const hasUrl = (source: Link | Pixel): source is Link => 'url' in source

/** The public path a source answers at. */
export const sourcePath = (kind: SourceKind, slug: string) => `/${kind === 'link' ? 'q' : 'p'}/${slug}`

/**
 * Links and pixels of a workspace. Their reports are explorations whose state names the link or
 * pixel as `site`, in UTC.
 */
export const linksResource = {
  links: (workspace: WorkspaceId, signal?: AbortSignal) => api.query('links.list', { workspace }, { signal }),
  pixels: (workspace: WorkspaceId, signal?: AbortSignal) => api.query('pixels.list', { workspace }, { signal }),
  createLink: (args: LinksCreateArgs, idempotencyKey?: string) => api.command('links.create', args, { idempotencyKey }),
  createPixel: (args: PixelsCreateArgs, idempotencyKey?: string) => api.command('pixels.create', args, { idempotencyKey }),
  updateLink: (args: LinksUpdateArgs, idempotencyKey?: string) => api.command('links.update', args, { idempotencyKey }),
  updatePixel: (args: PixelsUpdateArgs, idempotencyKey?: string) => api.command('pixels.update', args, { idempotencyKey }),
  /** Deletes the link and erases its hits. */
  removeLink: (link: LinkId) => api.command('links.delete', { link }),
  removePixel: (pixel: PixelId) => api.command('pixels.delete', { pixel }),
  explore: (kind: SourceKind, state: ExplorationState, panels: Panel[], signal?: AbortSignal) =>
    api.query(
      kind === 'link' ? 'links.explore' : 'pixels.explore',
      { state: JSON.stringify(state), panels: panels.map((panel) => JSON.stringify(panel)) },
      { signal }
    ),
}
