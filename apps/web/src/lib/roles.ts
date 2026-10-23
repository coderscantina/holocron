import type { InstallRole, InvitationState, Role, WorkspaceKind } from '~/generated'
import type { MessageKey } from '~/plugins/i18n'

/** Workspace roles from the broadest down. */
export const roles: Role[] = ['owner', 'admin', 'member', 'viewer']

export const roleLabels: Record<Role, MessageKey> = {
  owner: 'roles.owner',
  admin: 'roles.admin',
  member: 'roles.member',
  viewer: 'roles.viewer',
}

export const installRoles: InstallRole[] = ['user', 'install_admin']

export const installRoleLabels: Record<InstallRole, MessageKey> = {
  user: 'account.roles.user',
  install_admin: 'account.roles.install_admin',
}

export const kindLabels: Record<WorkspaceKind, MessageKey> = {
  team: 'context.kind.team',
  personal: 'context.kind.personal',
}

export const invitationStateLabels: Record<InvitationState, MessageKey> = {
  pending: 'invitations.states.pending',
  used: 'invitations.states.used',
  revoked: 'invitations.states.revoked',
  expired: 'invitations.states.expired',
}

/**
 * Roles the caller may hand out. Only owners make owners (decision 0012); the server checks
 * again, this only keeps impossible choices out of the list.
 */
export const assignableRoles = (own: Role | undefined): Role[] =>
  own === 'owner' ? roles : roles.filter((role) => role !== 'owner')
