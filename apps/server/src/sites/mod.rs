//! Sites and workspaces: `sites.*` and `workspaces.*` operations.

mod analytics;
mod manage;
pub mod policy;
pub mod scope;
mod tracking;

use chrono_tz::Tz;
use holocron_analytics::identity::{
    DEFAULT_WINDOW_MINUTES, MAX_TIME_MS, PolicyError, SessionPolicy, SitePolicies,
};
use holocron_protocol::access::{Capability, Role, WorkspaceKind};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_protocol::limits::{SITE_DOMAIN_MAX_CHARS, SITE_NAME_MAX_CHARS};
use holocron_protocol::operations::sites::{
    IdentityPolicy, RetentionPreview, Site, SiteDetail, SitesGetArgs, SitesHandlers,
    SitesUpdateArgs, SitesUpdateIdentityPolicyArgs, SitesUpdateResult, SitesUpdateRetentionArgs,
    SitesUpdateSessionPolicyArgs,
};
use holocron_protocol::operations::workspaces::{
    SiteSummary, WorkspaceList, WorkspaceSummary, WorkspacesHandlers,
};
use holocron_protocol::registry::Snapshot;
use holocron_storage::{Record, Revocation};
use serde_json::json;
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::consents;
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::retention::{self, Kind};
use crate::state::AppState;
use scope::SiteScope;

/// A scheduled policy change may take effect at most this far ahead.
const MAX_SCHEDULE_MS: i64 = 366 * 86_400_000;

impl SitesHandlers for AppState {
    async fn sites_get(
        &self,
        ctx: &mut QueryCtx,
        args: SitesGetArgs,
    ) -> Result<Snapshot<SiteDetail>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let site = load(ctx.conn(), &scope).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SiteDetail { site },
        })
    }

    async fn sites_update(
        &self,
        ctx: &mut CommandCtx,
        args: SitesUpdateArgs,
    ) -> Result<SitesUpdateResult, ApiError> {
        let scope = ctx
            .site_scope(
                &args.site,
                Capability::SiteConfigure,
                args.expected_revision,
            )
            .await?;
        let name = args.name.trim();
        let domain = args.domain.trim().to_lowercase();
        validate(name, &domain)?;

        sqlx::query("UPDATE sites SET name = ?, domain = ? WHERE id = ?")
            .bind(name)
            .bind(&domain)
            .bind(scope.site().as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.updated",
            json!({ "name": name, "domain": domain }),
        )
        .await?;

        let site = load(ctx.conn(), &scope).await?;
        Ok(SitesUpdateResult { site })
    }

    async fn sites_update_session_policy(
        &self,
        ctx: &mut CommandCtx,
        args: SitesUpdateSessionPolicyArgs,
    ) -> Result<SitesUpdateResult, ApiError> {
        let scope = ctx
            .site_scope(
                &args.site,
                Capability::SiteConfigure,
                args.expected_revision,
            )
            .await?;
        let invalid = |field: &str| FieldErrors::one(field, FieldErrorCode::InvalidValue, None);
        let timezone: Tz = args.timezone.parse().map_err(|_| invalid("timezone"))?;
        let policy = SessionPolicy::new(timezone, args.window_minutes)
            .map_err(|_| invalid("window_minutes"))?;
        let site = scope.site().as_str();
        let mut policies = site_policies(ctx.conn(), site).await?;
        // Identity days follow the collection clock, which tests can pin (0006).
        let now = self.collector.clock().now_ms();
        let effective = args.effective_at.unwrap_or(now);
        let retention = retention::effective(&mut *ctx.conn(), Kind::Site, site)
            .await?
            .ok_or_else(ApiError::not_found)?;
        if policies
            .retention_cutoff_ms(now, retention)
            .is_some_and(|cutoff| effective < cutoff)
            || effective > now.saturating_add(MAX_SCHEDULE_MS).min(MAX_TIME_MS)
        {
            return Err(invalid("effective_at"));
        }
        let version = match policies.change(policy, effective, now) {
            Ok(version) => version,
            Err(PolicyError::Unchanged) => {
                return Ok(SitesUpdateResult {
                    site: load(ctx.conn(), &scope).await?,
                });
            }
            Err(PolicyError::NotAfterCurrent | PolicyError::RetroactiveTimezone) => {
                return Err(invalid("effective_at"));
            }
            Err(PolicyError::Window(_)) => return Err(invalid("window_minutes")),
        };

        policy::insert_session_policy(ctx.conn(), site, &version, now).await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.session_policy_changed",
            json!({
                "version": version.version,
                "timezone": version.policy.timezone().name(),
                "window_minutes": version.policy.window_minutes(),
                "effective_at": version.effective_ms,
            }),
        )
        .await?;
        policy::mirror(ctx.conn(), self.collector.storage(), site)
            .await
            .map_err(ApiError::internal)?;
        self.correction.notify_one();
        Ok(SitesUpdateResult {
            site: load(ctx.conn(), &scope).await?,
        })
    }

    async fn sites_identity_policy(
        &self,
        ctx: &mut QueryCtx,
        args: SitesGetArgs,
    ) -> Result<Snapshot<IdentityPolicy>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        let (persistent_identity, identity_policy_version) =
            policy::identity_policy(ctx.conn(), scope.site().as_str()).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: IdentityPolicy {
                persistent_identity,
                identity_policy_version,
            },
        })
    }

    async fn sites_update_identity_policy(
        &self,
        ctx: &mut CommandCtx,
        args: SitesUpdateIdentityPolicyArgs,
    ) -> Result<IdentityPolicy, ApiError> {
        let scope = ctx
            .site_scope(
                &args.site,
                Capability::SiteConfigure,
                args.expected_revision,
            )
            .await?;
        let site = scope.site().as_str();
        let (enabled, version) = policy::identity_policy(ctx.conn(), site).await?;
        let mut answer = IdentityPolicy {
            persistent_identity: enabled,
            identity_policy_version: version,
        };
        if enabled == args.persistent_identity {
            return Ok(answer);
        }
        answer = IdentityPolicy {
            persistent_identity: args.persistent_identity,
            identity_policy_version: version + 1,
        };
        sqlx::query(
            "UPDATE sites SET persistent_identity = ?, identity_policy_version = ? WHERE id = ?",
        )
        .bind(answer.persistent_identity)
        .bind(answer.identity_policy_version)
        .bind(site)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.identity_policy_changed",
            json!(answer),
        )
        .await?;

        if !answer.persistent_identity {
            // Turning it off revokes every profile at this instant (0001). The revocation is
            // admitted before the command commits; a failed admission rolls the change back.
            let now = self.collector.clock().now_ms();
            consents::revoke_all(ctx.conn(), site, now).await?;
            let revocation = Record::Revocation(Revocation {
                id: Uuid::new_v4().as_u128(),
                site: policy::analytics_id(site).map_err(ApiError::internal)?,
                profile_id: None,
                revoked_at_ms: now,
                receive_time_ms: now,
            });
            self.collector
                .storage()
                .admit(&[revocation])
                .await
                .map_err(|refusal| {
                    ApiError::internal(format!("revocation not admitted: {refusal}"))
                })?;
        }
        Ok(answer)
    }

    async fn sites_update_retention(
        &self,
        ctx: &mut CommandCtx,
        args: SitesUpdateRetentionArgs,
    ) -> Result<SitesUpdateResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDelete, None)
            .await?;
        let site = scope.site().as_str();
        let own = retention::own(ctx.conn(), site).await?;
        let default = retention::installation_default(&mut *ctx.conn()).await?;
        let (current, next) = (own.unwrap_or(default), args.retention.unwrap_or(default));
        if next < current {
            ctx.require_fresh_login()?;
        }
        ctx.check_revision(&Topic::site(scope.site()), args.expected_revision)
            .await?;
        if own != args.retention {
            sqlx::query("UPDATE sites SET event_retention = ? WHERE id = ?")
                .bind(args.retention.map(|r| r.code()))
                .bind(site)
                .execute(ctx.conn())
                .await?;
            ctx.record(
                &Topic::site(scope.site()),
                "site.retention_changed",
                json!({ "retention": args.retention, "effective": next }),
            )
            .await?;
        }
        Ok(SitesUpdateResult {
            site: load(ctx.conn(), &scope).await?,
        })
    }

    async fn sites_retention_preview(
        &self,
        ctx: &mut QueryCtx,
        args: SitesUpdateRetentionArgs,
    ) -> Result<Snapshot<RetentionPreview>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteDelete).await?;
        let site = scope.site().as_str();
        let own = retention::own(ctx.conn(), site).await?;
        let default = retention::installation_default(&mut *ctx.conn()).await?;
        let source = retention::Source {
            id: site.to_owned(),
            kind: Kind::Site,
            own,
            policies: site_policies(ctx.conn(), site).await?,
        };
        let current = source.retention(default);
        let preview = self
            .retention_preview(vec![(source, current)], args.retention.unwrap_or(default))
            .await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: preview,
        })
    }
}

/// Workspace id, name, kind and role, then the site id, name and domain when it has sites.
type MembershipRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

impl WorkspacesHandlers for AppState {
    async fn workspaces_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<WorkspaceList>, ApiError> {
        let user = ctx.person()?;
        let rows: Vec<MembershipRow> = sqlx::query_as(
            "SELECT w.id, w.name, w.kind, m.role, s.id, s.name, s.domain
                 FROM memberships m
                 JOIN workspaces w ON w.id = m.workspace_id
                 LEFT JOIN sites s ON s.workspace_id = w.id
                 WHERE m.user_id = ?
                 ORDER BY w.name COLLATE NOCASE, w.id, s.name COLLATE NOCASE, s.id",
        )
        .bind(user.as_str())
        .fetch_all(ctx.conn())
        .await?;

        let mut workspaces: Vec<WorkspaceSummary> = Vec::new();
        for (id, name, kind, role, site_id, site_name, site_domain) in rows {
            if workspaces.last().is_none_or(|w| w.id.as_str() != id) {
                workspaces.push(WorkspaceSummary {
                    id: WorkspaceId::new(id),
                    name,
                    kind: WorkspaceKind::parse(&kind).unwrap_or(WorkspaceKind::Team),
                    role: Role::parse(&role).unwrap_or(Role::Viewer),
                    sites: Vec::new(),
                });
            }
            if let (Some(site), Some(name), Some(domain), Some(workspace)) =
                (site_id, site_name, site_domain, workspaces.last_mut())
            {
                workspace.sites.push(SiteSummary {
                    id: SiteId::new(site),
                    name,
                    domain,
                });
            }
        }
        Ok(Snapshot {
            topic: Topic::installation(),
            data: WorkspaceList { workspaces },
        })
    }
}

async fn load(conn: &mut SqliteConnection, scope: &SiteScope) -> Result<Site, ApiError> {
    let (name, domain): (String, String) =
        sqlx::query_as("SELECT name, domain FROM sites WHERE id = ?")
            .bind(scope.site().as_str())
            .fetch_one(&mut *conn)
            .await?;
    let retention_override = retention::own(&mut *conn, scope.site().as_str()).await?;
    let retention =
        retention_override.unwrap_or(retention::installation_default(&mut *conn).await?);
    let policies = site_policies(conn, scope.site().as_str()).await?;
    let current = policies.current();
    Ok(Site {
        retention,
        retention_override,
        id: scope.site().clone(),
        workspace: scope.workspace().clone(),
        name,
        domain,
        timezone: current.policy.timezone().name().to_owned(),
        session_window_minutes: current.policy.window_minutes(),
        session_policy_version: current.version,
        capabilities: scope.capabilities(),
    })
}

/// Session policy versions of a site the caller already resolved.
async fn site_policies(conn: &mut SqliteConnection, site: &str) -> Result<SitePolicies, ApiError> {
    policy::session_policies(conn, site)
        .await?
        .ok_or_else(|| ApiError::internal(format!("site {site} has no session policy")))
}

fn validate(name: &str, domain: &str) -> Result<(), ApiError> {
    let mut errors = FieldErrors::default();
    let name_chars = name.chars().count();
    if name_chars == 0 {
        errors.add("name", FieldErrorCode::Required, None);
    } else if name_chars > SITE_NAME_MAX_CHARS {
        errors.add(
            "name",
            FieldErrorCode::TooLong,
            Some(SITE_NAME_MAX_CHARS as u32),
        );
    }

    if domain.is_empty() {
        errors.add("domain", FieldErrorCode::Required, None);
    } else if domain.len() > SITE_DOMAIN_MAX_CHARS {
        errors.add(
            "domain",
            FieldErrorCode::TooLong,
            Some(SITE_DOMAIN_MAX_CHARS as u32),
        );
    } else if !is_host_name(domain) {
        errors.add("domain", FieldErrorCode::InvalidFormat, None);
    }
    errors.finish()
}

/// Dot-separated labels of letters, digits and inner hyphens, like `roadhouse.test` or `localhost`.
fn is_host_name(value: &str) -> bool {
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Creates a site with session policy version 1: `timezone` and the default inactivity window.
/// Used by seeding and tests until a `sites.create` operation exists. Analytics gets the policy
/// through [`policy::mirror`], or at the next start.
pub async fn create_site(
    conn: &mut SqliteConnection,
    workspace: &WorkspaceId,
    name: &str,
    domain: &str,
    timezone: &str,
) -> anyhow::Result<SiteId> {
    let timezone: Tz = timezone
        .parse()
        .map_err(|_| anyhow::anyhow!("unknown timezone {timezone}"))?;
    let first = SitePolicies::new(SessionPolicy::new(timezone, DEFAULT_WINDOW_MINUTES)?);
    let id = crate::db::new_id();
    let now = crate::db::now_ms();
    sqlx::query(
        "INSERT INTO sites (id, workspace_id, name, domain, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(workspace.as_str())
    .bind(name)
    .bind(domain)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    policy::insert_session_policy(conn, &id, first.current(), now).await?;
    Ok(SiteId::new(id))
}

pub async fn create_workspace(
    conn: &mut SqliteConnection,
    name: &str,
    kind: WorkspaceKind,
) -> Result<WorkspaceId, sqlx::Error> {
    let id = crate::db::new_id();
    sqlx::query("INSERT INTO workspaces (id, name, kind, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(name)
        .bind(kind.as_str())
        .bind(crate::db::now_ms())
        .execute(conn)
        .await?;
    Ok(WorkspaceId::new(id))
}

pub async fn add_member(
    conn: &mut SqliteConnection,
    workspace: &WorkspaceId,
    user: &UserId,
    role: Role,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES (?, ?, ?)")
        .bind(workspace.as_str())
        .bind(user.as_str())
        .bind(role.as_str())
        .execute(conn)
        .await?;
    Ok(())
}
