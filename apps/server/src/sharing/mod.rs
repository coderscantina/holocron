//! Restricted shares: the viewer principal, its reach and the limits a viewer's request must stay
//! inside. `sharing.*` handlers are in [`operations`].
//!
//! The dispatcher already refused operations the share does not reach (registry `ShareReach`),
//! and site scopes refuse sites outside the share. What remains for handlers is the widening
//! check: [`admit_exploration`] for states and panels, [`admit_field`] for value suggestions.

mod limits;
mod operations;

pub use limits::{admit_exploration, admit_field};

use holocron_protocol::ids::{ShareId, SiteId, WorkspaceId};
use holocron_protocol::operations::sharing::ShareTarget;
use sqlx::SqliteConnection;

use crate::error::ApiError;

/// The share behind an `Authorization: Share <token>` request, loaded once per request.
#[derive(Debug, Clone)]
pub struct ShareViewer {
    pub id: ShareId,
    pub workspace: WorkspaceId,
    pub target: ShareTarget,
    pub export: bool,
    pub recordings: bool,
    pub expires_at: i64,
}

impl ShareViewer {
    /// Whether the share names `site`. A site moved to another workspace is outside it too,
    /// which the site scope checks against [`ShareViewer::workspace`].
    pub fn reaches(&self, site: &SiteId) -> bool {
        match &self.target {
            ShareTarget::Exploration { state, .. } => &state.site == site,
            ShareTarget::Board { sites, .. } => sites.contains(site),
        }
    }
}

/// The live share with this token digest: not revoked and not expired at `now`.
pub async fn viewer(
    conn: &mut SqliteConnection,
    token_hash: &str,
    now: i64,
) -> Result<Option<ShareViewer>, ApiError> {
    let row: Option<(String, String, String, bool, bool, i64)> = sqlx::query_as(
        "SELECT id, workspace_id, target, export, recordings, expires_at FROM shares
         WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(conn)
    .await?;
    let Some((id, workspace, target, export, recordings, expires_at)) = row else {
        return Ok(None);
    };
    let target: ShareTarget = serde_json::from_str(&target).map_err(ApiError::internal)?;
    Ok(Some(ShareViewer {
        id: ShareId::new(id),
        workspace: WorkspaceId::new(workspace),
        target,
        export,
        recordings,
        expires_at,
    }))
}
