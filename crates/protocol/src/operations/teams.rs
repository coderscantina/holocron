//! Teams, their members and invitations (CAP-ACCESS-08, CAP-ACCESS-09). A team is a workspace of
//! kind `team`. A workspace the caller is not a member of answers `not_found`, a missing
//! capability `forbidden` (decision order in `fixtures/access/README.md`).
//!
//! Invitations work without email: `teams.invite` discloses a single-use code once, an admin
//! passes it on (link or text), and a signed-in person redeems it with `teams.join`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::{Role, WorkspaceCapability, WorkspaceKind};
use crate::envelope::Empty;
use crate::ids::{InvitationId, UserId, WorkspaceId};
use crate::operations::explorations::Deleted;

crate::domain! {
    pub trait TeamsHandlers;
    commands {
        /// Creates a team with the caller as its only owner.
        "teams.create" => teams_create (Person): TeamsCreateArgs => TeamResult;
        /// Renames a team. Needs `team.manage`.
        "teams.update" => teams_update (Person): TeamsUpdateArgs => TeamResult;
        /// Deletes a team with its sites, boards, shares and invitations. Owner only (R-OWNER-ONLY)
        /// and needs reauthentication. Personal workspaces cannot be deleted (`not_found`).
        "teams.delete" => teams_delete (Person, reauth): TeamArgs => Deleted;
        /// Creates a single-use invitation for one role and discloses its code once. Needs
        /// `team.manage`, and the role must be below the caller's own, except that owners may
        /// invite owners (R-RANK). Lifetime 1 to 30 days, 7 by default.
        "teams.invite" => teams_invite (Person, secret): TeamsInviteArgs => InvitationCreated;
        /// Revokes a pending invitation. Needs `team.manage`.
        "teams.revokeInvitation" => teams_revoke_invitation (Person): InvitationArgs => InvitationResult;
        /// Redeems an invitation code and adds the caller with its role. Unknown, used, revoked
        /// and expired codes answer the same `not_found`. Already a member: the role stays and
        /// the code stays unused.
        "teams.join" => teams_join (Person): TeamsJoinArgs => TeamResult;
        /// Changes a member's role. Needs `team.manage`; the caller must outrank the member and
        /// the new role must be below the caller's own, except that owners may make owners and
        /// change other owners' roles (R-RANK). Nobody changes their own role, so a team keeps an
        /// owner. Raising to admin or owner needs reauthentication (decision 0007).
        "members.updateRole" => members_update_role (Person, reauth_by_handler): MembersUpdateRoleArgs => TeamResult;
        /// Removes a member. Any member may remove themselves (R-SELF-LEAVE); removing someone
        /// else needs `team.manage` and a higher rank. The last owner cannot leave.
        "members.remove" => members_remove (Person): MemberArgs => Deleted;
    }
    queries {
        /// Teams the caller is a member of by name; install admins get every team, with
        /// `role` absent where they are not members.
        "teams.list" => teams_list (Person): Empty => TeamList;
        /// One team with its members. Needs `workspace.read`.
        "teams.get" => teams_get (Person): TeamArgs => TeamDetail;
        /// Pending, used, revoked and expired invitations, newest first. Needs `team.manage`.
        "teams.invitations" => teams_invitations (Person): TeamArgs => InvitationList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Team {
    pub id: WorkspaceId,
    pub name: String,
    pub kind: WorkspaceKind,
    /// The caller's role. Absent for an install admin who is not a member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role: Option<Role>,
    /// What the caller may do in the team.
    pub capabilities: Vec<WorkspaceCapability>,
    pub two_factor_required: bool,
    pub members: i64,
    pub sites: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Member {
    pub user: UserId,
    pub name: String,
    pub email: String,
    pub role: Role,
    pub two_factor_enrolled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamList {
    pub teams: Vec<Team>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamArgs {
    pub workspace: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamResult {
    pub team: Team,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamDetail {
    pub team: Team,
    /// Members by rank, then name.
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamsCreateArgs {
    /// 1 to 100 characters after trimming.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamsUpdateArgs {
    pub workspace: WorkspaceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvitationState {
    Pending,
    Used,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Invitation {
    pub id: InvitationId,
    pub workspace: WorkspaceId,
    pub role: Role,
    /// Who it is meant for, so admins can tell invitations apart. 0 to 100 characters.
    pub label: String,
    /// The first characters of the code.
    pub prefix: String,
    pub state: InvitationState,
    pub created_at: i64,
    pub expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub used_by: Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamsInviteArgs {
    pub workspace: WorkspaceId,
    pub role: Role,
    #[serde(default)]
    pub label: String,
    /// 1 to 30. Defaults to 7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lifetime_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationCreated {
    pub invitation: Invitation,
    /// The invitation code. The join link is `/teams/join?code=<code>`. `null` in a replay.
    pub secret: Option<String>,
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationArgs {
    pub invitation: InvitationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationResult {
    pub invitation: Invitation,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationList {
    pub invitations: Vec<Invitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamsJoinArgs {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MembersUpdateRoleArgs {
    pub workspace: WorkspaceId,
    pub user: UserId,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MemberArgs {
    pub workspace: WorkspaceId,
    pub user: UserId,
}
