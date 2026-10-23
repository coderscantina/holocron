//! `users.*` operations: installation administration of accounts and erasure.

use holocron_protocol::access::{InstallRole, Role, WorkspaceKind};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode, OwnedResource};
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_protocol::operations::users::{
    CredentialsReset, UserArgs, UserCreated, UserDeleted, UserDetail, UserList, UserMembership,
    UserResult, UserSummary, UsersCreateArgs, UsersHandlers, UsersResetCredentialsArgs,
    UsersUpdateArgs,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::{
    end_sessions, generate_password, hash_password_blocking, two_factor, validate_profile,
};
use crate::context::{CommandCtx, QueryCtx};
use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

/// The columns of a [`SummaryRow`] from `users u`.
macro_rules! select_summary {
    ($rest:literal) => {
        concat!(
            "SELECT u.id, u.email, u.name, u.install_role, u.two_factor_required, u.created_at,
                    EXISTS (SELECT 1 FROM two_factor t WHERE t.user_id = u.id AND t.secret IS NOT NULL)
                        AS two_factor_enrolled
             FROM users u ",
            $rest
        )
    };
}

impl UsersHandlers for AppState {
    async fn users_create(
        &self,
        ctx: &mut CommandCtx,
        args: UsersCreateArgs,
    ) -> Result<UserCreated, ApiError> {
        ctx.require_install_admin().await?;
        let (name, email) = validate_profile(ctx.conn(), &args.name, &args.email, None).await?;
        let password = generate_password();
        let hash = hash_password_blocking(password.clone()).await?;
        let user = UserId::new(new_id());
        sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, install_role, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(user.as_str())
        .bind(&email)
        .bind(&name)
        .bind(hash)
        .bind(args.install_role.as_str())
        .bind(ctx.now())
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::installation(),
            "user.created",
            json!({ "user": user }),
        )
        .await?;
        Ok(UserCreated {
            user: summary(ctx.conn(), &user).await?,
            secret: Some(password),
            secret_withheld: false,
        })
    }

    async fn users_update(
        &self,
        ctx: &mut CommandCtx,
        args: UsersUpdateArgs,
    ) -> Result<UserResult, ApiError> {
        ctx.require_install_admin().await?;
        let current = install_role(ctx.conn(), &args.user).await?;
        ctx.check_revision(&Topic::installation(), args.expected_revision)
            .await?;
        let (name, email) =
            validate_profile(ctx.conn(), &args.name, &args.email, Some(&args.user)).await?;
        if current == InstallRole::InstallAdmin
            && args.install_role != InstallRole::InstallAdmin
            && install_admins(ctx.conn()).await? == 1
        {
            return Err(ApiError::new(ErrorCode::LastInstallAdmin));
        }
        sqlx::query("UPDATE users SET name = ?, email = ?, install_role = ? WHERE id = ?")
            .bind(&name)
            .bind(&email)
            .bind(args.install_role.as_str())
            .bind(args.user.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::installation(),
            "user.updated",
            json!({ "user": args.user }),
        )
        .await?;
        Ok(UserResult {
            user: summary(ctx.conn(), &args.user).await?,
        })
    }

    async fn users_reset_credentials(
        &self,
        ctx: &mut CommandCtx,
        args: UsersResetCredentialsArgs,
    ) -> Result<CredentialsReset, ApiError> {
        ctx.require_install_admin().await?;
        install_role(ctx.conn(), &args.user).await?;
        ctx.require_fresh_login()?;
        if !args.password && !args.two_factor {
            return Err(FieldErrors::one("password", FieldErrorCode::Required, None));
        }
        let password = if args.password {
            let password = generate_password();
            let hash = hash_password_blocking(password.clone()).await?;
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
                .bind(hash)
                .bind(args.user.as_str())
                .execute(ctx.conn())
                .await?;
            Some(password)
        } else {
            None
        };
        if args.two_factor {
            two_factor::remove(ctx.conn(), &args.user).await?;
        }
        end_sessions(ctx.conn(), &args.user, None).await?;
        ctx.record(
            &Topic::installation(),
            "user.credentials_reset",
            json!({ "user": args.user, "password": args.password, "two_factor": args.two_factor }),
        )
        .await?;
        Ok(CredentialsReset {
            user: summary(ctx.conn(), &args.user).await?,
            secret: password,
            secret_withheld: false,
        })
    }

    async fn users_delete(
        &self,
        ctx: &mut CommandCtx,
        args: UserArgs,
    ) -> Result<UserDeleted, ApiError> {
        ctx.require_install_admin().await?;
        let role = install_role(ctx.conn(), &args.user).await?;
        ctx.require_fresh_login()?;
        if role == InstallRole::InstallAdmin && install_admins(ctx.conn()).await? == 1 {
            return Err(ApiError::new(ErrorCode::LastInstallAdmin));
        }
        let owned = owned_resources(ctx.conn(), &args.user).await?;
        if !owned.blocking.is_empty() {
            return Err(ApiError::user_owns_resources(owned.blocking));
        }

        let now = ctx.now();
        let conn = ctx.conn();
        // Stored command answers may hold account data; keys go with the user below.
        sqlx::query(
            "DELETE FROM idempotency_keys WHERE owner = 'user:' || ?1
                OR owner IN (SELECT 'key:' || id FROM api_keys WHERE owner_id = ?1)",
        )
        .bind(args.user.as_str())
        .execute(&mut *conn)
        .await?;
        for workspace in &owned.empty_personal {
            sqlx::query("DELETE FROM workspaces WHERE id = ?")
                .bind(workspace.as_str())
                .execute(&mut *conn)
                .await?;
        }
        // Cascades to sessions, memberships, own explorations, the second factor, recovery
        // codes, keys and their grants, invitations the user created, challenges and
        // preferences. Workspace content keeps its `created_by` ID, resolved by the tombstone.
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(args.user.as_str())
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO user_tombstones (user_id, deleted_at) VALUES (?, ?)")
            .bind(args.user.as_str())
            .bind(now)
            .execute(&mut *conn)
            .await?;
        ctx.record(
            &Topic::installation(),
            "user.deleted",
            json!({ "user": args.user }),
        )
        .await?;
        Ok(UserDeleted {
            user: args.user,
            deleted_at: now,
        })
    }

    async fn users_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<UserList>, ApiError> {
        ctx.require_install_admin().await?;
        let rows: Vec<SummaryRow> = sqlx::query_as(select_summary!("ORDER BY u.email"))
            .fetch_all(ctx.conn())
            .await?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: UserList {
                users: rows.into_iter().map(SummaryRow::into_summary).collect(),
            },
        })
    }

    async fn users_get(
        &self,
        ctx: &mut QueryCtx,
        args: UserArgs,
    ) -> Result<Snapshot<UserDetail>, ApiError> {
        ctx.require_install_admin().await?;
        let user = summary(ctx.conn(), &args.user).await?;
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT w.id, w.name, w.kind, m.role FROM memberships m
               JOIN workspaces w ON w.id = m.workspace_id
             WHERE m.user_id = ? ORDER BY w.name, w.id",
        )
        .bind(args.user.as_str())
        .fetch_all(ctx.conn())
        .await?;
        let memberships = rows
            .into_iter()
            .map(|(workspace, name, kind, role)| {
                Ok(UserMembership {
                    workspace: WorkspaceId::new(workspace),
                    name,
                    kind: WorkspaceKind::parse(&kind)
                        .ok_or_else(|| ApiError::internal(format!("unknown kind {kind}")))?,
                    role: Role::parse(&role)
                        .ok_or_else(|| ApiError::internal(format!("unknown role {role}")))?,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: UserDetail { user, memberships },
        })
    }
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: String,
    email: String,
    name: String,
    install_role: String,
    two_factor_required: bool,
    created_at: i64,
    two_factor_enrolled: bool,
}

impl SummaryRow {
    fn into_summary(self) -> UserSummary {
        UserSummary {
            id: UserId::new(self.id),
            email: self.email,
            name: self.name,
            install_role: InstallRole::parse(&self.install_role).unwrap_or(InstallRole::User),
            two_factor_enrolled: self.two_factor_enrolled,
            two_factor_required: self.two_factor_required,
            created_at: self.created_at,
        }
    }
}

/// One account, `not_found` when it does not exist.
async fn summary(conn: &mut SqliteConnection, user: &UserId) -> Result<UserSummary, ApiError> {
    let row: Option<SummaryRow> = sqlx::query_as(select_summary!("WHERE u.id = ?"))
        .bind(user.as_str())
        .fetch_optional(conn)
        .await?;
    row.map(SummaryRow::into_summary)
        .ok_or_else(ApiError::not_found)
}

/// The target's installation role, `not_found` when it does not exist.
async fn install_role(conn: &mut SqliteConnection, user: &UserId) -> Result<InstallRole, ApiError> {
    let role: Option<String> = sqlx::query_scalar("SELECT install_role FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_optional(conn)
        .await?;
    role.as_deref()
        .map(|role| InstallRole::parse(role).unwrap_or(InstallRole::User))
        .ok_or_else(ApiError::not_found)
}

async fn install_admins(conn: &mut SqliteConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE install_role = 'install_admin'")
        .fetch_one(conn)
        .await
}

struct Owned {
    /// Teams the user is the only owner of, and sites of their personal workspaces.
    blocking: Vec<OwnedResource>,
    /// Personal workspaces without sites, erased with the user.
    empty_personal: Vec<WorkspaceId>,
}

/// What erasing `user` would leave without an owner (decision 0007 User erasure scope).
async fn owned_resources(conn: &mut SqliteConnection, user: &UserId) -> Result<Owned, ApiError> {
    /// Workspace ID, name and kind, with each site of a personal workspace.
    type Row = (String, String, String, Option<String>, Option<String>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT w.id, w.name, w.kind, s.id, s.name FROM memberships m
           JOIN workspaces w ON w.id = m.workspace_id
           LEFT JOIN sites s ON s.workspace_id = w.id AND w.kind = 'personal'
         WHERE m.user_id = ?1 AND m.role = 'owner'
           AND NOT EXISTS (SELECT 1 FROM memberships o
                           WHERE o.workspace_id = w.id AND o.role = 'owner' AND o.user_id <> ?1)
         ORDER BY w.name, w.id, s.name, s.id",
    )
    .bind(user.as_str())
    .fetch_all(conn)
    .await?;
    let mut owned = Owned {
        blocking: Vec::new(),
        empty_personal: Vec::new(),
    };
    for (workspace, name, kind, site, site_name) in rows {
        match (kind.as_str(), site, site_name) {
            ("personal", Some(site), Some(site_name)) => {
                owned.blocking.push(OwnedResource::Site {
                    id: SiteId::new(site),
                    name: site_name,
                });
            }
            ("personal", ..) => owned.empty_personal.push(WorkspaceId::new(workspace)),
            _ => owned.blocking.push(OwnedResource::Workspace {
                id: WorkspaceId::new(workspace),
                name,
            }),
        }
    }
    Ok(owned)
}
