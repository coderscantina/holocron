//! Authorized scopes. Only the resolvers here construct a [`SiteScope`] or [`WorkspaceScope`],
//! so code holding one knows authorization already happened for this caller on this resource.
//!
//! Every principal resolves through the same functions: a person through membership, an API key
//! through its owner's membership limited to the key's sites, a share viewer through the
//! share's sites with read (plus export and recordings when granted) and nothing else.

use std::sync::Arc;

use holocron_protocol::access::{
    Capability, InstallRole, Role, WorkspaceCapability, WorkspaceKind,
};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_protocol::operations::api_keys::KeySites;
use sqlx::SqliteConnection;

use crate::error::ApiError;
use crate::identity::Caller;
use crate::sharing::ShareViewer;

#[derive(Debug, Clone)]
pub struct SiteScope {
    site: SiteId,
    workspace: WorkspaceId,
    capabilities: Vec<Capability>,
    share: Option<Arc<ShareViewer>>,
}

impl SiteScope {
    pub fn site(&self) -> &SiteId {
        &self.site
    }

    pub fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    /// The share this scope was resolved through. Handlers that read rows (recordings,
    /// sessions) must keep to its range and pinned filter.
    pub fn share(&self) -> Option<&ShareViewer> {
        self.share.as_deref()
    }

    /// `forbidden` when the role lacks the capability; the site is already known to be visible.
    /// A share viewer gets `not_found` instead: outside a share everything is hidden.
    pub fn require(&self, capability: Capability) -> Result<(), ApiError> {
        if self.can(capability) {
            Ok(())
        } else if self.share.is_some() {
            Err(ApiError::not_found())
        } else {
            Err(ApiError::new(ErrorCode::Forbidden))
        }
    }
}

/// Resolves `user`'s scope on `site` through workspace membership. A missing site, a site in
/// another workspace and a site of a workspace the user left all answer the same `not_found`.
/// Install admins get no implicit access (decision 0007).
pub async fn resolve(
    conn: &mut SqliteConnection,
    user: &UserId,
    site: &SiteId,
) -> Result<SiteScope, ApiError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT s.workspace_id, m.role FROM sites s
         JOIN memberships m ON m.workspace_id = s.workspace_id AND m.user_id = ?
         WHERE s.id = ?",
    )
    .bind(user.as_str())
    .bind(site.as_str())
    .fetch_optional(conn)
    .await?;

    let (workspace, role) = row.ok_or_else(ApiError::not_found)?;
    let role = parse_role(&role)?;
    Ok(SiteScope {
        site: site.clone(),
        workspace: WorkspaceId::new(workspace),
        capabilities: role.site_capabilities().to_vec(),
        share: None,
    })
}

/// Resolves any caller's scope on `site`. A key reaches only its listed sites, with its owner's
/// current role (R-KEY-SITES, R-KEY-CEILING). A share reaches only its sites inside its
/// workspace. Everything outside answers `not_found`.
pub async fn resolve_for(
    conn: &mut SqliteConnection,
    caller: &Caller,
    site: &SiteId,
) -> Result<SiteScope, ApiError> {
    match caller {
        Caller::Person { user, .. } => resolve(conn, user, site).await,
        Caller::Key(key) => {
            if !key.reaches(site) {
                return Err(ApiError::not_found());
            }
            resolve(conn, &key.owner, site).await
        }
        Caller::Share { viewer, .. } => {
            if !viewer.reaches(site) {
                return Err(ApiError::not_found());
            }
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sites WHERE id = ? AND workspace_id = ?")
                    .bind(site.as_str())
                    .bind(viewer.workspace.as_str())
                    .fetch_optional(conn)
                    .await?;
            exists.ok_or_else(ApiError::not_found)?;
            let mut capabilities = vec![Capability::SiteRead];
            if viewer.export {
                capabilities.push(Capability::SiteExport);
            }
            if viewer.recordings {
                capabilities.push(Capability::SiteRecordingsRead);
            }
            Ok(SiteScope {
                site: site.clone(),
                workspace: viewer.workspace.clone(),
                capabilities,
                share: Some(Arc::clone(viewer)),
            })
        }
        Caller::Anonymous { .. } => Err(ApiError::new(ErrorCode::Unauthenticated)),
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceScope {
    workspace: WorkspaceId,
    kind: WorkspaceKind,
    role: Role,
}

impl WorkspaceScope {
    pub fn workspace(&self) -> &WorkspaceId {
        &self.workspace
    }

    pub fn kind(&self) -> WorkspaceKind {
        self.kind
    }

    /// The acting person's role, for rank checks (R-RANK, R-OWNER-ONLY).
    pub fn role(&self) -> Role {
        self.role
    }

    pub fn can(&self, capability: WorkspaceCapability) -> bool {
        self.role.workspace_capabilities().contains(&capability)
    }

    /// `forbidden` when the role lacks the capability; the workspace is already visible.
    pub fn require(&self, capability: WorkspaceCapability) -> Result<(), ApiError> {
        if self.can(capability) {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::Forbidden))
        }
    }

    /// What the role may do with the workspace's links and pixels: what it may do with one of
    /// its sites (decision 0020).
    pub fn source_capabilities(&self) -> &'static [Capability] {
        self.role.site_capabilities()
    }

    /// `forbidden` when the role lacks `capability` on the workspace's links and pixels.
    pub fn require_source(&self, capability: Capability) -> Result<(), ApiError> {
        if self.source_capabilities().contains(&capability) {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::Forbidden))
        }
    }
}

/// Resolves `user`'s membership in `workspace`. Missing and not-a-member answer the same
/// `not_found`. Install admins get no implicit membership.
pub async fn resolve_workspace(
    conn: &mut SqliteConnection,
    user: &UserId,
    workspace: &WorkspaceId,
) -> Result<WorkspaceScope, ApiError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT w.kind, m.role FROM workspaces w
         JOIN memberships m ON m.workspace_id = w.id AND m.user_id = ?
         WHERE w.id = ?",
    )
    .bind(user.as_str())
    .bind(workspace.as_str())
    .fetch_optional(conn)
    .await?;
    let (kind, role) = row.ok_or_else(ApiError::not_found)?;
    let kind = WorkspaceKind::parse(&kind)
        .ok_or_else(|| ApiError::internal(format!("unknown workspace kind {kind}")))?;
    Ok(WorkspaceScope {
        workspace: workspace.clone(),
        kind,
        role: parse_role(&role)?,
    })
}

/// Resolves any caller's membership scope. Only keys with `KeySites::All` reach workspace-level
/// resources: a key limited to listed sites would otherwise read boards and members beyond
/// them. Share viewers never do.
pub async fn resolve_workspace_for(
    conn: &mut SqliteConnection,
    caller: &Caller,
    workspace: &WorkspaceId,
) -> Result<WorkspaceScope, ApiError> {
    match caller {
        Caller::Person { user, .. } => resolve_workspace(conn, user, workspace).await,
        Caller::Key(key) if key.sites == KeySites::All => {
            resolve_workspace(conn, &key.owner, workspace).await
        }
        Caller::Key(_) | Caller::Share { .. } => Err(ApiError::not_found()),
        Caller::Anonymous { .. } => Err(ApiError::new(ErrorCode::Unauthenticated)),
    }
}

/// `forbidden` unless the caller is a signed-in install admin, read now so a demotion applies
/// at once (AX-S016). Returns the admin's ID.
pub async fn require_install_admin(
    conn: &mut SqliteConnection,
    caller: &Caller,
) -> Result<UserId, ApiError> {
    let user = caller
        .user()
        .ok_or_else(|| ApiError::new(ErrorCode::Forbidden))?;
    let role: Option<String> = sqlx::query_scalar("SELECT install_role FROM users WHERE id = ?")
        .bind(user.as_str())
        .fetch_optional(conn)
        .await?;
    match role.as_deref().and_then(InstallRole::parse) {
        Some(InstallRole::InstallAdmin) => Ok(user.clone()),
        _ => Err(ApiError::new(ErrorCode::Forbidden)),
    }
}

fn parse_role(role: &str) -> Result<Role, ApiError> {
    Role::parse(role).ok_or_else(|| ApiError::internal(format!("unknown role {role}")))
}
