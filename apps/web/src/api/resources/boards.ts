import type { BoardsUpdateArgs, ExplorationState, WorkspaceId } from '~/generated'

import { api } from '..'

/** Workspace boards. `revision` of a board snapshot is the workspace topic's head. */
export const boardsResource = {
  list: (workspace: WorkspaceId, signal?: AbortSignal) => api.query('boards.list', { workspace }, { signal }),
  get: (board: string, signal?: AbortSignal) => api.query('boards.get', { board }, { signal }),
  create: (workspace: WorkspaceId, name: string) => api.command('boards.create', { workspace, name }),
  update: (args: BoardsUpdateArgs) => api.command('boards.update', args),
  pin: (board: string, title: string, state: ExplorationState, exploration?: string) =>
    api.command('boards.pin', { board, title, state, exploration }),
  remove: (board: string) => api.command('boards.delete', { board }),
}
