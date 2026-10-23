//! Teams, members and invitations: `teams.*` and `members.*` operations. Membership scopes
//! come from [`crate::sites::scope::resolve_workspace_for`] through `CommandCtx::workspace_scope`.
//!
//! Ranks follow R-RANK: an actor changes or removes only members it outranks and grants only
//! roles below its own, except that owners may make owners and change other owners' roles
//! (decision 0012). Nobody changes their own role and the last owner cannot leave, so no team
//! is left without one.

use holocron_protocol::access::{Role, WorkspaceCapability, WorkspaceKind};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::{InvitationId, UserId, WorkspaceId};
use holocron_protocol::limits::{
    ACCOUNT_NAME_MAX_CHARS, INVITATION_LIFETIME_MAX_DAYS, LABEL_MAX_CHARS,
};
use holocron_protocol::operations::explorations::Deleted;
use holocron_protocol::operations::teams::{
    Invitation, InvitationArgs, InvitationCreated, InvitationList, InvitationResult,
    InvitationState, Member, MemberArgs, MembersUpdateRoleArgs, Team, TeamArgs, TeamDetail,
    TeamList, TeamResult, TeamsCreateArgs, TeamsHandlers, TeamsInviteArgs, TeamsJoinArgs,
    TeamsUpdateArgs,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity;
use crate::sites;
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;
const DEFAULT_INVITATION_DAYS: u32 = 7;

impl TeamsHandlers for AppState {
    async fn teams_create(
        &self,
        ctx: &mut CommandCtx,
        args: TeamsCreateArgs,
    ) -> Result<TeamResult, ApiError> {
        let user = ctx.person()?;
        let name = team_name(&args.name)?;
        let workspace = sites::create_workspace(ctx.conn(), &name, WorkspaceKind::Team).await?;
        sites::add_member(ctx.conn(), &workspace, &user, Role::Owner).await?;
        ctx.workspace_scope(&workspace, WorkspaceCapability::Read, None)
            .await?;
        ctx.record(&Topic::workspace(&workspace), "team.created", json!({}))
            .await?;
        Ok(TeamResult {
            team: team(ctx.conn(), &workspace, &user).await?,
        })
    }

    async fn teams_update(
        &self,
        ctx: &mut CommandCtx,
        args: TeamsUpdateArgs,
    ) -> Result<TeamResult, ApiError> {
        let user = ctx.person()?;
        ctx.workspace_scope(
            &args.workspace,
            WorkspaceCapability::Manage,
            args.expected_revision,
        )
        .await?;
        let name = team_name(&args.name)?;
        sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
            .bind(&name)
            .bind(args.workspace.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&args.workspace),
            "team.renamed",
            json!({ "name": name }),
        )
        .await?;
        Ok(TeamResult {
            team: team(ctx.conn(), &args.workspace, &user).await?,
        })
    }

    async fn teams_delete(
        &self,
        ctx: &mut CommandCtx,
        args: TeamArgs,
    ) -> Result<Deleted, ApiError> {
        let scope = ctx
            .workspace_scope(&args.workspace, WorkspaceCapability::Manage, None)
            .await?;
        if scope.kind() == WorkspaceKind::Personal {
            return Err(ApiError::not_found());
        }
        if scope.role() != Role::Owner {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        ctx.require_fresh_login()?;
        // Sites, boards, shares, invitations and memberships go with the workspace.
        sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(args.workspace.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::installation(),
            "team.deleted",
            json!({ "id": args.workspace }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn teams_invite(
        &self,
        ctx: &mut CommandCtx,
        args: TeamsInviteArgs,
    ) -> Result<InvitationCreated, ApiError> {
        let scope = ctx
            .workspace_scope(&args.workspace, WorkspaceCapability::Manage, None)
            .await?;
        if scope.kind() == WorkspaceKind::Personal {
            return Err(FieldErrors::one(
                "workspace",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        if !may_grant(scope.role(), args.role) {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        let mut errors = FieldErrors::default();
        let label = args.label.trim();
        if label.chars().count() > LABEL_MAX_CHARS {
            errors.add(
                "label",
                FieldErrorCode::TooLong,
                Some(LABEL_MAX_CHARS as u32),
            );
        }
        let days = args.lifetime_days.unwrap_or(DEFAULT_INVITATION_DAYS);
        if !(1..=INVITATION_LIFETIME_MAX_DAYS).contains(&days) {
            errors.add("lifetime_days", FieldErrorCode::InvalidValue, None);
        }
        errors.finish()?;

        let user = ctx.person()?;
        let id = InvitationId::new(crate::db::new_id());
        let (code, digest) = identity::new_session_token();
        let now = ctx.now();
        sqlx::query(
            "INSERT INTO invitations (id, workspace_id, role, label, prefix, code_hash, created_by, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(args.workspace.as_str())
        .bind(args.role.as_str())
        .bind(label)
        .bind(&code[..8])
        .bind(digest)
        .bind(user.as_str())
        .bind(now)
        .bind(now + i64::from(days) * DAY_MS)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::workspace(&args.workspace),
            "invitation.created",
            json!({ "id": id }),
        )
        .await?;
        Ok(InvitationCreated {
            invitation: invitation(ctx.conn(), &id, now).await?,
            secret: Some(code),
            secret_withheld: false,
        })
    }

    async fn teams_revoke_invitation(
        &self,
        ctx: &mut CommandCtx,
        args: InvitationArgs,
    ) -> Result<InvitationResult, ApiError> {
        let workspace: Option<String> =
            sqlx::query_scalar("SELECT workspace_id FROM invitations WHERE id = ?")
                .bind(args.invitation.as_str())
                .fetch_optional(ctx.conn())
                .await?;
        let workspace = WorkspaceId::new(workspace.ok_or_else(ApiError::not_found)?);
        ctx.workspace_scope(&workspace, WorkspaceCapability::Manage, None)
            .await?;
        let now = ctx.now();
        let revoked = sqlx::query(
            "UPDATE invitations SET revoked_at = ?
             WHERE id = ? AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
        )
        .bind(now)
        .bind(args.invitation.as_str())
        .bind(now)
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if revoked > 0 {
            ctx.record(
                &Topic::workspace(&workspace),
                "invitation.revoked",
                json!({ "id": args.invitation }),
            )
            .await?;
        }
        Ok(InvitationResult {
            invitation: invitation(ctx.conn(), &args.invitation, now).await?,
        })
    }

    async fn teams_join(
        &self,
        ctx: &mut CommandCtx,
        args: TeamsJoinArgs,
    ) -> Result<TeamResult, ApiError> {
        let user = ctx.person()?;
        let now = ctx.now();
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT id, workspace_id, role FROM invitations
             WHERE code_hash = ? AND used_at IS NULL AND revoked_at IS NULL AND expires_at > ?",
        )
        .bind(identity::token_digest(args.code.trim()))
        .bind(now)
        .fetch_optional(ctx.conn())
        .await?;
        let (id, workspace, role) = row.ok_or_else(ApiError::not_found)?;
        let workspace = WorkspaceId::new(workspace);
        if member_role(ctx.conn(), &workspace, &user).await?.is_none() {
            let role = parse_role(&role)?;
            sites::add_member(ctx.conn(), &workspace, &user, role).await?;
            sqlx::query("UPDATE invitations SET used_by = ?, used_at = ? WHERE id = ?")
                .bind(user.as_str())
                .bind(now)
                .bind(&id)
                .execute(ctx.conn())
                .await?;
            ctx.record(
                &Topic::workspace(&workspace),
                "member.joined",
                json!({ "user": user, "invitation": id }),
            )
            .await?;
        }
        ctx.workspace_scope(&workspace, WorkspaceCapability::Read, None)
            .await?;
        Ok(TeamResult {
            team: team(ctx.conn(), &workspace, &user).await?,
        })
    }

    async fn members_update_role(
        &self,
        ctx: &mut CommandCtx,
        args: MembersUpdateRoleArgs,
    ) -> Result<TeamResult, ApiError> {
        let actor = ctx.person()?;
        let scope = ctx
            .workspace_scope(&args.workspace, WorkspaceCapability::Manage, None)
            .await?;
        let current = member_role(ctx.conn(), &args.workspace, &args.user)
            .await?
            .ok_or_else(ApiError::not_found)?;
        if args.user == actor
            || !may_change(scope.role(), current)
            || !may_grant(scope.role(), args.role)
        {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }
        if matches!(args.role, Role::Admin | Role::Owner) {
            ctx.require_fresh_login()?;
        }
        let topic = Topic::workspace(&args.workspace);
        ctx.check_revision(&topic, args.expected_revision).await?;
        if current != args.role {
            sqlx::query("UPDATE memberships SET role = ? WHERE workspace_id = ? AND user_id = ?")
                .bind(args.role.as_str())
                .bind(args.workspace.as_str())
                .bind(args.user.as_str())
                .execute(ctx.conn())
                .await?;
            ctx.record(
                &topic,
                "member.role_changed",
                json!({ "user": args.user, "role": args.role }),
            )
            .await?;
        }
        Ok(TeamResult {
            team: team(ctx.conn(), &args.workspace, &actor).await?,
        })
    }

    async fn members_remove(
        &self,
        ctx: &mut CommandCtx,
        args: MemberArgs,
    ) -> Result<Deleted, ApiError> {
        let actor = ctx.person()?;
        let scope = ctx
            .workspace_scope(&args.workspace, WorkspaceCapability::Read, None)
            .await?;
        let target = member_role(ctx.conn(), &args.workspace, &args.user)
            .await?
            .ok_or_else(ApiError::not_found)?;
        if args.user == actor {
            let owners: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM memberships WHERE workspace_id = ? AND role = 'owner'",
            )
            .bind(args.workspace.as_str())
            .fetch_one(ctx.conn())
            .await?;
            if target == Role::Owner && owners == 1 {
                return Err(FieldErrors::one("user", FieldErrorCode::InvalidValue, None));
            }
        } else {
            scope.require(WorkspaceCapability::Manage)?;
            if scope.role().rank() <= target.rank() {
                return Err(ApiError::new(ErrorCode::Forbidden));
            }
        }
        sqlx::query("DELETE FROM memberships WHERE workspace_id = ? AND user_id = ?")
            .bind(args.workspace.as_str())
            .bind(args.user.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::workspace(&args.workspace),
            "member.removed",
            json!({ "user": args.user }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn teams_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<TeamList>, ApiError> {
        let user = ctx.person()?;
        let rows: Vec<TeamRow> = sqlx::query_as(concat!(
            select_team!(),
            " WHERE m.user_id IS NOT NULL
                OR (w.kind = 'team' AND (SELECT install_role FROM users WHERE id = ?) = 'install_admin')
              ORDER BY w.name COLLATE NOCASE, w.id"
        ))
        .bind(user.as_str())
        .bind(user.as_str())
        .fetch_all(ctx.conn())
        .await?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: TeamList {
                teams: rows.into_iter().map(team_of).collect::<Result<_, _>>()?,
            },
        })
    }

    async fn teams_get(
        &self,
        ctx: &mut QueryCtx,
        args: TeamArgs,
    ) -> Result<Snapshot<TeamDetail>, ApiError> {
        let user = ctx.person()?;
        ctx.workspace_scope(&args.workspace, WorkspaceCapability::Read)
            .await?;
        let team = team(ctx.conn(), &args.workspace, &user).await?;
        let rows: Vec<(String, String, String, String, bool)> = sqlx::query_as(
            "SELECT u.id, u.name, u.email, m.role,
                    EXISTS (SELECT 1 FROM two_factor t WHERE t.user_id = u.id AND t.secret IS NOT NULL)
             FROM memberships m JOIN users u ON u.id = m.user_id
             WHERE m.workspace_id = ?
             ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 WHEN 'member' THEN 2 ELSE 3 END,
                      u.name COLLATE NOCASE, u.id",
        )
        .bind(args.workspace.as_str())
        .fetch_all(ctx.conn())
        .await?;
        let members = rows
            .into_iter()
            .map(|(id, name, email, role, two_factor_enrolled)| {
                Ok(Member {
                    user: UserId::new(id),
                    name,
                    email,
                    role: parse_role(&role)?,
                    two_factor_enrolled,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic: Topic::workspace(&args.workspace),
            data: TeamDetail { team, members },
        })
    }

    async fn teams_invitations(
        &self,
        ctx: &mut QueryCtx,
        args: TeamArgs,
    ) -> Result<Snapshot<InvitationList>, ApiError> {
        ctx.workspace_scope(&args.workspace, WorkspaceCapability::Manage)
            .await?;
        let rows: Vec<InvitationRow> = sqlx::query_as(concat!(
            select_invitation!(),
            " WHERE workspace_id = ? ORDER BY created_at DESC, id"
        ))
        .bind(args.workspace.as_str())
        .fetch_all(ctx.conn())
        .await?;
        let now = crate::db::now_ms();
        Ok(Snapshot {
            topic: Topic::workspace(&args.workspace),
            data: InvitationList {
                invitations: rows
                    .into_iter()
                    .map(|row| invitation_of(row, now))
                    .collect::<Result<_, _>>()?,
            },
        })
    }
}

/// R-RANK: roles below the actor's own, and owner by an owner (decision 0012).
fn may_grant(actor: Role, role: Role) -> bool {
    role.rank() < actor.rank() || (actor == Role::Owner && role == Role::Owner)
}

/// R-RANK: members the actor outranks, and other owners by an owner (decision 0012). Callers
/// refuse the actor's own role, so an owner who demotes another stays one.
fn may_change(actor: Role, member: Role) -> bool {
    member.rank() < actor.rank() || (actor == Role::Owner && member == Role::Owner)
}

fn team_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 {
        return Err(FieldErrors::one("name", FieldErrorCode::Required, None));
    }
    if length > ACCOUNT_NAME_MAX_CHARS {
        return Err(FieldErrors::one(
            "name",
            FieldErrorCode::TooLong,
            Some(ACCOUNT_NAME_MAX_CHARS as u32),
        ));
    }
    Ok(name.to_owned())
}

fn parse_role(role: &str) -> Result<Role, ApiError> {
    Role::parse(role).ok_or_else(|| ApiError::internal(format!("unknown role {role}")))
}

async fn member_role(
    conn: &mut SqliteConnection,
    workspace: &WorkspaceId,
    user: &UserId,
) -> Result<Option<Role>, ApiError> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT role FROM memberships WHERE workspace_id = ? AND user_id = ?")
            .bind(workspace.as_str())
            .bind(user.as_str())
            .fetch_optional(conn)
            .await?;
    role.as_deref().map(parse_role).transpose()
}

/// Teams with the role of the user bound first, if a member.
macro_rules! select_team {
    () => {
        "SELECT w.id, w.name, w.kind, m.role, w.two_factor_required,
                (SELECT count(*) FROM memberships c WHERE c.workspace_id = w.id),
                (SELECT count(*) FROM sites s WHERE s.workspace_id = w.id),
                w.created_at
         FROM workspaces w LEFT JOIN memberships m ON m.workspace_id = w.id AND m.user_id = ?"
    };
}
use select_team;

type TeamRow = (String, String, String, Option<String>, bool, i64, i64, i64);

async fn team(
    conn: &mut SqliteConnection,
    workspace: &WorkspaceId,
    user: &UserId,
) -> Result<Team, ApiError> {
    let row: TeamRow = sqlx::query_as(concat!(select_team!(), " WHERE w.id = ?"))
        .bind(user.as_str())
        .bind(workspace.as_str())
        .fetch_one(conn)
        .await?;
    team_of(row)
}

fn team_of(
    (id, name, kind, role, two_factor_required, members, sites, created_at): TeamRow,
) -> Result<Team, ApiError> {
    let role = role.as_deref().map(parse_role).transpose()?;
    Ok(Team {
        id: WorkspaceId::new(id),
        name,
        kind: WorkspaceKind::parse(&kind)
            .ok_or_else(|| ApiError::internal(format!("unknown workspace kind {kind}")))?,
        capabilities: role
            .map(|role| role.workspace_capabilities().to_vec())
            .unwrap_or_default(),
        role,
        two_factor_required,
        members,
        sites,
        created_at,
    })
}

macro_rules! select_invitation {
    () => {
        "SELECT id, workspace_id, role, label, prefix, created_at, expires_at, used_by, used_at,
                revoked_at
         FROM invitations"
    };
}
use select_invitation;

type InvitationRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

async fn invitation(
    conn: &mut SqliteConnection,
    id: &InvitationId,
    now: i64,
) -> Result<Invitation, ApiError> {
    let row: InvitationRow = sqlx::query_as(concat!(select_invitation!(), " WHERE id = ?"))
        .bind(id.as_str())
        .fetch_one(conn)
        .await?;
    invitation_of(row, now)
}

fn invitation_of(
    (id, workspace, role, label, prefix, created_at, expires_at, used_by, used_at, revoked_at): InvitationRow,
    now: i64,
) -> Result<Invitation, ApiError> {
    let state = if used_at.is_some() {
        InvitationState::Used
    } else if revoked_at.is_some() {
        InvitationState::Revoked
    } else if expires_at <= now {
        InvitationState::Expired
    } else {
        InvitationState::Pending
    };
    Ok(Invitation {
        id: InvitationId::new(id),
        workspace: WorkspaceId::new(workspace),
        role: parse_role(&role)?,
        label,
        prefix,
        state,
        created_at,
        expires_at,
        used_by: used_by.map(UserId::new),
    })
}
