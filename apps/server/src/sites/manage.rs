//! Site lifecycle: `sites.list`, `sites.create`, `sites.delete`, `sites.reset` and `sites.transfer`.
//!
//! Deleting a site removes its control rows at once (settings, definitions, boards' cards,
//! shares, key grants), so every principal gets `not_found` from then on, and queues the
//! ordered erasure of its records in the same transaction (decision 0014). A reset queues
//! the erasure of the records admitted before it and keeps the site.

use chrono_tz::Tz;
use holocron_protocol::access::{Capability, Role, WorkspaceCapability, WorkspaceKind};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_protocol::operations::explorations::Deleted;
use holocron_protocol::operations::site_management::{
    SiteList, SiteListItem, SiteManagementHandlers, SiteReset, SitesCreateArgs, SitesTransferArgs,
    TransferTarget,
};
use holocron_protocol::operations::sites::{SiteDetail, SitesGetArgs};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{add_member, create_site, create_workspace, load, policy, validate};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::maintenance::jobs::{self, Job};
use crate::state::AppState;

impl SiteManagementHandlers for AppState {
    async fn sites_create(
        &self,
        ctx: &mut CommandCtx,
        args: SitesCreateArgs,
    ) -> Result<SiteDetail, ApiError> {
        ctx.workspace_scope(&args.workspace, WorkspaceCapability::Write, None)
            .await?;
        let name = args.name.trim();
        let domain = args.domain.trim().to_lowercase();
        validate(name, &domain)?;
        let timezone: Tz = args
            .timezone
            .parse()
            .map_err(|_| FieldErrors::one("timezone", FieldErrorCode::InvalidValue, None))?;
        let site = create_site(ctx.conn(), &args.workspace, name, &domain, timezone.name())
            .await
            .map_err(ApiError::internal)?;
        policy::mirror(ctx.conn(), self.collector.storage(), site.as_str())
            .await
            .map_err(ApiError::internal)?;
        ctx.record(
            &Topic::workspace(&args.workspace),
            "site.created",
            json!({ "id": site }),
        )
        .await?;
        let scope = ctx.site_scope(&site, Capability::SiteRead, None).await?;
        Ok(SiteDetail {
            site: load(ctx.conn(), &scope).await?,
        })
    }

    async fn sites_delete(
        &self,
        ctx: &mut CommandCtx,
        args: SitesGetArgs,
    ) -> Result<Deleted, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDelete, None)
            .await?;
        ctx.require_fresh_login()?;
        let site = Uuid::parse_str(scope.site().as_str()).map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM sites WHERE id = ?")
            .bind(scope.site().as_str())
            .execute(ctx.conn())
            .await?;
        // Committed with the deletion, so the erasure runs even after a crash right after it.
        let now = ctx.now();
        jobs::enqueue(ctx.conn(), &Job::EraseSite { site }, now).await?;
        self.maintenance.wake.notify_one();
        ctx.record(
            &Topic::workspace(scope.workspace()),
            "site.deleted",
            json!({ "id": scope.site() }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn sites_reset(
        &self,
        ctx: &mut CommandCtx,
        args: SitesGetArgs,
    ) -> Result<SiteReset, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDelete, None)
            .await?;
        ctx.require_fresh_login()?;
        let site = Uuid::parse_str(scope.site().as_str()).map_err(ApiError::internal)?;
        let now = ctx.now();
        // Records admitted from here on are the site's new data, whenever the job runs.
        let through_seq = self
            .collector
            .storage()
            .last_sequence()
            .await
            .map_err(ApiError::internal)?;
        jobs::enqueue(ctx.conn(), &Job::ResetSite { site, through_seq }, now).await?;
        self.maintenance.wake.notify_one();
        ctx.record(
            &Topic::site(scope.site()),
            "site.reset",
            json!({ "reset_at": now }),
        )
        .await?;
        Ok(SiteReset {
            site: args.site,
            reset_at: now,
        })
    }

    async fn sites_transfer(
        &self,
        ctx: &mut CommandCtx,
        args: SitesTransferArgs,
    ) -> Result<SiteDetail, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteTransfer, None)
            .await?;
        let source = scope.workspace().clone();
        let target = match &args.to {
            TransferTarget::ToTeam { team } => {
                let target = ctx
                    .workspace_scope(team, WorkspaceCapability::Write, None)
                    .await?;
                if target.kind() != WorkspaceKind::Team {
                    return Err(FieldErrors::one("to", FieldErrorCode::InvalidValue, None));
                }
                team.clone()
            }
            TransferTarget::ToUser { user } => {
                let role = ctx
                    .workspace_scope(&source, WorkspaceCapability::Read, None)
                    .await?
                    .role();
                if role != Role::Owner {
                    return Err(ApiError::new(ErrorCode::Forbidden));
                }
                personal_workspace(ctx.conn(), &source, user).await?
            }
        };
        if target == source {
            return Err(FieldErrors::one("to", FieldErrorCode::InvalidValue, None));
        }
        ctx.require_fresh_login()?;

        let site = scope.site().as_str();
        sqlx::query("UPDATE sites SET workspace_id = ? WHERE id = ?")
            .bind(target.as_str())
            .bind(site)
            .execute(ctx.conn())
            .await?;
        // The site's shares and key grants end, and cards of the old workspace's boards go:
        // a card must be a site of its board's workspace.
        sqlx::query("UPDATE shares SET revoked_at = ? WHERE site_id = ? AND revoked_at IS NULL")
            .bind(ctx.now())
            .bind(site)
            .execute(ctx.conn())
            .await?;
        sqlx::query("DELETE FROM api_key_sites WHERE site_id = ?")
            .bind(site)
            .execute(ctx.conn())
            .await?;
        sqlx::query("DELETE FROM board_cards WHERE site_id = ?")
            .bind(site)
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.transferred",
            json!({ "from": source, "to": target }),
        )
        .await?;

        let mut detail = load(ctx.conn(), &scope).await?;
        let person = ctx.person()?;
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(target.as_str())
        .bind(person.as_str())
        .fetch_optional(ctx.conn())
        .await?;
        detail.capabilities = role
            .as_deref()
            .and_then(Role::parse)
            .map(|role| role.site_capabilities().to_vec())
            .unwrap_or_default();
        detail.workspace = target;
        Ok(SiteDetail { site: detail })
    }

    async fn sites_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<SiteList>, ApiError> {
        let user = ctx.person()?;
        let rows: Vec<(String, String, String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT s.id, s.name, s.domain, w.id, w.name, m.role
             FROM sites s JOIN workspaces w ON w.id = s.workspace_id
             LEFT JOIN memberships m ON m.workspace_id = w.id AND m.user_id = ?
             WHERE m.user_id IS NOT NULL
                OR (SELECT install_role FROM users WHERE id = ?) = 'install_admin'
             ORDER BY w.name COLLATE NOCASE, w.id, s.name COLLATE NOCASE, s.id",
        )
        .bind(user.as_str())
        .bind(user.as_str())
        .fetch_all(ctx.conn())
        .await?;
        let sites = rows
            .into_iter()
            .map(
                |(id, name, domain, workspace, workspace_name, role)| SiteListItem {
                    id: SiteId::new(id),
                    name,
                    domain,
                    workspace: WorkspaceId::new(workspace),
                    workspace_name,
                    role: role.as_deref().and_then(Role::parse),
                },
            )
            .collect();
        Ok(Snapshot {
            topic: Topic::installation(),
            data: SiteList { sites },
        })
    }
}

/// The personal workspace of `user`, created on first use. The user must be a member of the
/// site's current workspace; anyone else is `not_found`, like a missing user.
async fn personal_workspace(
    conn: &mut SqliteConnection,
    source: &WorkspaceId,
    user: &UserId,
) -> Result<WorkspaceId, ApiError> {
    let name: Option<String> = sqlx::query_scalar(
        "SELECT u.name FROM memberships m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = ? AND m.user_id = ?",
    )
    .bind(source.as_str())
    .bind(user.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let name = name.ok_or_else(ApiError::not_found)?;
    let personal: Option<String> = sqlx::query_scalar(
        "SELECT w.id FROM workspaces w JOIN memberships m ON m.workspace_id = w.id
         WHERE w.kind = 'personal' AND m.user_id = ? AND m.role = 'owner'
         ORDER BY w.created_at LIMIT 1",
    )
    .bind(user.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(personal) = personal {
        return Ok(WorkspaceId::new(personal));
    }
    let workspace = create_workspace(conn, &name, WorkspaceKind::Personal).await?;
    add_member(conn, &workspace, user, Role::Owner).await?;
    Ok(workspace)
}
