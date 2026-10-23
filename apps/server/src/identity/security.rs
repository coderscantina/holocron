//! `security.*` operations: 2FA requirements at installation, team and user level. Whether a
//! session is enrollment-only is derived from these on every request, never stored.

use holocron_protocol::access::{InstallRole, Role, WorkspaceCapability, WorkspaceKind};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::ids::{UserId, WorkspaceId};
use holocron_protocol::operations::security::{
    PolicyTarget, SecurityHandlers, SecurityPolicy, SecurityUpdatePolicyArgs, TeamPolicy,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::signed_in;
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

impl SecurityHandlers for AppState {
    async fn security_update_policy(
        &self,
        ctx: &mut CommandCtx,
        args: SecurityUpdatePolicyArgs,
    ) -> Result<SecurityPolicy, ApiError> {
        let sql = match &args.target {
            PolicyTarget::Installation => {
                ctx.require_install_admin().await?;
                "UPDATE installation_settings SET two_factor_required = ?1, updated_at = ?2
                 WHERE id = 1 AND ?3 IS NULL AND two_factor_required <> ?1"
            }
            PolicyTarget::User { user } => {
                ctx.require_install_admin().await?;
                let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id = ?")
                    .bind(user.as_str())
                    .fetch_optional(ctx.conn())
                    .await?;
                exists.ok_or_else(ApiError::not_found)?;
                "UPDATE users SET two_factor_required = ?1 WHERE id = ?3 AND two_factor_required <> ?1"
            }
            PolicyTarget::Team { workspace } => {
                let scope = ctx
                    .workspace_scope(workspace, WorkspaceCapability::Manage, None)
                    .await?;
                if scope.kind() != WorkspaceKind::Team {
                    return Err(FieldErrors::one(
                        "target",
                        FieldErrorCode::InvalidValue,
                        None,
                    ));
                }
                "UPDATE workspaces SET two_factor_required = ?1 WHERE id = ?3 AND two_factor_required <> ?1"
            }
        };
        ctx.require_fresh_login()?;

        let id = match &args.target {
            PolicyTarget::Installation => None,
            PolicyTarget::User { user } => Some(user.as_str().to_owned()),
            PolicyTarget::Team { workspace } => Some(workspace.as_str().to_owned()),
        };
        let changed = sqlx::query(sql)
            .bind(args.two_factor_required)
            .bind(ctx.now())
            .bind(id)
            .execute(ctx.conn())
            .await?
            .rows_affected()
            > 0;
        if changed {
            let topic = match &args.target {
                PolicyTarget::Team { workspace } => Topic::workspace(workspace),
                _ => Topic::installation(),
            };
            ctx.record(
                &topic,
                "security.policy_changed",
                json!({ "target": args.target, "two_factor_required": args.two_factor_required }),
            )
            .await?;
        }
        let user = signed_in(ctx.caller())?;
        policy(ctx.conn(), &user).await
    }

    async fn security_policy(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<SecurityPolicy>, ApiError> {
        let user = signed_in(ctx.caller())?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: policy(ctx.conn(), &user).await?,
        })
    }
}

/// The requirements `user` may see: the installation's, and each team they manage (every team
/// for install admins).
async fn policy(conn: &mut SqliteConnection, user: &UserId) -> Result<SecurityPolicy, ApiError> {
    let (installation_two_factor_required, install_role): (bool, String) = sqlx::query_as(
        "SELECT (SELECT two_factor_required FROM installation_settings WHERE id = 1), install_role
         FROM users WHERE id = ?",
    )
    .bind(user.as_str())
    .fetch_one(&mut *conn)
    .await?;
    let can_manage_installation =
        InstallRole::parse(&install_role) == Some(InstallRole::InstallAdmin);
    let teams: Vec<(String, String, bool, Option<String>)> = sqlx::query_as(
        "SELECT w.id, w.name, w.two_factor_required, m.role FROM workspaces w
           LEFT JOIN memberships m ON m.workspace_id = w.id AND m.user_id = ?
         WHERE w.kind = 'team' ORDER BY w.name, w.id",
    )
    .bind(user.as_str())
    .fetch_all(conn)
    .await?;
    let manages = |role: Option<&str>| {
        role.and_then(Role::parse).is_some_and(|role| {
            role.workspace_capabilities()
                .contains(&WorkspaceCapability::Manage)
        })
    };
    Ok(SecurityPolicy {
        installation_two_factor_required,
        can_manage_installation,
        teams: teams
            .into_iter()
            .filter(|(.., role)| can_manage_installation || manages(role.as_deref()))
            .map(|(workspace, name, two_factor_required, _)| TeamPolicy {
                workspace: WorkspaceId::new(workspace),
                name,
                two_factor_required,
            })
            .collect(),
    })
}
