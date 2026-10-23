import type {
  InvitationId,
  MembersUpdateRoleArgs,
  TeamsInviteArgs,
  TeamsUpdateArgs,
  UserId,
  WorkspaceId,
} from '~/generated'

import { api } from '..'

/** Teams, their members and invitations. */
export const teamsResource = {
  list: (signal?: AbortSignal) => api.query('teams.list', {}, { signal }),
  get: (workspace: WorkspaceId, signal?: AbortSignal) => api.query('teams.get', { workspace }, { signal }),
  invitations: (workspace: WorkspaceId, signal?: AbortSignal) =>
    api.query('teams.invitations', { workspace }, { signal }),
  create: (name: string, idempotencyKey?: string) => api.command('teams.create', { name }, { idempotencyKey }),
  update: (args: TeamsUpdateArgs, idempotencyKey?: string) => api.command('teams.update', args, { idempotencyKey }),
  remove: (workspace: WorkspaceId) => api.command('teams.delete', { workspace }),
  /** Discloses the invitation code once, in `secret`. */
  invite: (args: TeamsInviteArgs, idempotencyKey?: string) => api.command('teams.invite', args, { idempotencyKey }),
  revokeInvitation: (invitation: InvitationId) => api.command('teams.revokeInvitation', { invitation }),
  join: (code: string) => api.command('teams.join', { code }),
  updateRole: (args: MembersUpdateRoleArgs) => api.command('members.updateRole', args),
  removeMember: (workspace: WorkspaceId, user: UserId) => api.command('members.remove', { workspace, user }),
}
