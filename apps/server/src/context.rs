//! What a command or query handler receives besides its arguments.
//!
//! The dispatcher has already applied steps 1 to 3 of the access decision order (credential,
//! key grants, share reach, enrollment-only sessions). Handlers apply the rest through these
//! helpers, in this order: resolve the scope (`not_found`), require the capability
//! (`forbidden`), handler-specific rank or owner checks (`forbidden`), then
//! [`CommandCtx::require_fresh_login`] for sensitive commands (`reauthentication_required`),
//! then the expected revision (`stale_revision`).

use holocron_protocol::access::{Capability, InstallRole, WorkspaceCapability};
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::exploration::{ExplorationState, Panel};
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_protocol::registry::{Handler, OperationInfo, Reauth};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::error::ApiError;
use crate::events;
use crate::identity::Caller;
use crate::sharing;
use crate::sites::scope::{self, SiteScope, WorkspaceScope};
use crate::state::AppState;

impl Handler for AppState {
    type CommandCtx = CommandCtx;
    type QueryCtx = QueryCtx;
    type Error = ApiError;
}

/// A scope a command resolved. Stored with its idempotent answer and resolved again before a
/// replay is returned. Untagged, so answers stored before milestone 10 still read as `Site`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Authorized {
    Site {
        site: SiteId,
        capability: Capability,
    },
    Workspace {
        workspace: WorkspaceId,
        workspace_capability: WorkspaceCapability,
    },
    /// The links and pixels of a workspace (decision 0020).
    Sources {
        workspace: WorkspaceId,
        source_capability: Capability,
    },
    Installation {
        install_role: InstallRole,
    },
}

/// Resolves every stored scope again with the caller's current membership and capabilities.
pub(crate) async fn reauthorize(
    conn: &mut SqliteConnection,
    caller: &Caller,
    authorized: &[Authorized],
) -> Result<(), ApiError> {
    for entry in authorized {
        match entry {
            Authorized::Site { site, capability } => scope::resolve_for(&mut *conn, caller, site)
                .await?
                .require(*capability)?,
            Authorized::Workspace {
                workspace,
                workspace_capability,
            } => scope::resolve_workspace_for(&mut *conn, caller, workspace)
                .await?
                .require(*workspace_capability)?,
            Authorized::Sources {
                workspace,
                source_capability,
            } => scope::resolve_workspace_for(&mut *conn, caller, workspace)
                .await?
                .require_source(*source_capability)?,
            Authorized::Installation { .. } => {
                scope::require_install_admin(&mut *conn, caller).await?;
            }
        }
    }
    Ok(())
}

/// A change to the caller's cookies, applied to the response after commit.
pub enum SessionChange {
    /// Sets the session cookie and clears any login challenge.
    Start(String),
    /// Clears the session cookie.
    End,
    /// Sets the login challenge cookie between password and second factor.
    Challenge(String),
}

/// One command: its write transaction, the caller and what it did.
pub struct CommandCtx {
    tx: Transaction<'static, Sqlite>,
    caller: Caller,
    now: i64,
    op: &'static OperationInfo,
    pub(crate) last_event: Option<i64>,
    pub(crate) authorized: Vec<Authorized>,
    pub(crate) session: Option<SessionChange>,
    /// The handler passed [`CommandCtx::require_fresh_login`].
    pub(crate) fresh_login: bool,
}

impl CommandCtx {
    pub fn new(
        tx: Transaction<'static, Sqlite>,
        caller: Caller,
        now: i64,
        op: &'static OperationInfo,
    ) -> Self {
        Self {
            tx,
            caller,
            now,
            op,
            last_event: None,
            authorized: Vec::new(),
            session: None,
            fresh_login: false,
        }
    }

    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn now(&self) -> i64 {
        self.now
    }

    /// The person whose capabilities apply: the signed-in person or the API key's owner.
    pub fn person(&self) -> Result<UserId, ApiError> {
        acting_user(&self.caller)
    }

    /// Resolves the caller's scope on `site` and requires `capability`, then checks the
    /// expected revision. A hidden site is `not_found`, a missing capability `forbidden`, and
    /// only a caller allowed to write learns the current revision. For a reauthenticated
    /// command with a revision, the fresh-login check runs before the revision.
    pub async fn site_scope(
        &mut self,
        site: &SiteId,
        capability: Capability,
        expected_revision: Option<i64>,
    ) -> Result<SiteScope, ApiError> {
        let scope = scope::resolve_for(&mut self.tx, &self.caller, site).await?;
        scope.require(capability)?;
        self.authorized.push(Authorized::Site {
            site: site.clone(),
            capability,
        });
        self.before_revision(&Topic::site(site), expected_revision)
            .await?;
        Ok(scope)
    }

    /// The workspace counterpart of [`CommandCtx::site_scope`], with the revision of the
    /// workspace topic.
    pub async fn workspace_scope(
        &mut self,
        workspace: &WorkspaceId,
        capability: WorkspaceCapability,
        expected_revision: Option<i64>,
    ) -> Result<WorkspaceScope, ApiError> {
        let scope = scope::resolve_workspace_for(&mut self.tx, &self.caller, workspace).await?;
        scope.require(capability)?;
        self.authorized.push(Authorized::Workspace {
            workspace: workspace.clone(),
            workspace_capability: capability,
        });
        self.before_revision(&Topic::workspace(workspace), expected_revision)
            .await?;
        Ok(scope)
    }

    /// Resolves the caller's scope on the links and pixels of `workspace` and requires
    /// `capability` on them.
    pub async fn sources_scope(
        &mut self,
        workspace: &WorkspaceId,
        capability: Capability,
    ) -> Result<WorkspaceScope, ApiError> {
        let scope = scope::resolve_workspace_for(&mut self.tx, &self.caller, workspace).await?;
        scope.require_source(capability)?;
        self.authorized.push(Authorized::Sources {
            workspace: workspace.clone(),
            source_capability: capability,
        });
        Ok(scope)
    }

    /// `forbidden` unless the caller is a signed-in install admin. Returns the admin's ID.
    pub async fn require_install_admin(&mut self) -> Result<UserId, ApiError> {
        let user = scope::require_install_admin(&mut self.tx, &self.caller).await?;
        self.authorized.push(Authorized::Installation {
            install_role: InstallRole::InstallAdmin,
        });
        Ok(user)
    }

    /// `reauthentication_required` unless this login session passed a password check within
    /// the last 10 minutes. Call after every scope, capability and rank check, before the
    /// revision. Operations declared `reauth` must call it (directly or through a scope helper
    /// with a revision); the dispatcher refuses to commit otherwise.
    pub fn require_fresh_login(&mut self) -> Result<(), ApiError> {
        self.caller.require_fresh_login(self.now)?;
        self.fresh_login = true;
        Ok(())
    }

    /// `stale_revision` with the current head when `expected` is older than `topic`.
    pub async fn check_revision(
        &mut self,
        topic: &Topic,
        expected: Option<i64>,
    ) -> Result<(), ApiError> {
        if let Some(expected) = expected {
            let head = events::head(&mut self.tx, topic).await?;
            if expected != head {
                return Err(ApiError::stale_revision(head));
            }
        }
        Ok(())
    }

    async fn before_revision(
        &mut self,
        topic: &Topic,
        expected: Option<i64>,
    ) -> Result<(), ApiError> {
        if expected.is_some() && self.op.policy.reauth == Reauth::Always {
            self.require_fresh_login()?;
        }
        self.check_revision(topic, expected).await
    }

    /// Records an event in this command's transaction. The last one becomes the revision.
    pub async fn record(
        &mut self,
        topic: &Topic,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<i64, ApiError> {
        let seq = events::record(&mut self.tx, topic, kind, &payload, self.now).await?;
        self.last_event = Some(seq);
        Ok(seq)
    }

    pub fn start_session(&mut self, token: String) {
        self.session = Some(SessionChange::Start(token));
    }

    pub fn end_session(&mut self) {
        self.session = Some(SessionChange::End);
    }

    pub fn start_challenge(&mut self, token: String) {
        self.session = Some(SessionChange::Challenge(token));
    }

    pub(crate) fn into_parts(self) -> CommandParts {
        CommandParts {
            tx: self.tx,
            last_event: self.last_event,
            authorized: self.authorized,
            session: self.session,
            fresh_login: self.fresh_login,
        }
    }
}

/// What the dispatcher needs after the handler ran.
pub(crate) struct CommandParts {
    pub tx: Transaction<'static, Sqlite>,
    pub last_event: Option<i64>,
    pub authorized: Vec<Authorized>,
    pub session: Option<SessionChange>,
    pub fresh_login: bool,
}

/// One query: a read transaction, so the snapshot and its topic head agree.
pub struct QueryCtx {
    tx: Transaction<'static, Sqlite>,
    caller: Caller,
}

impl QueryCtx {
    pub fn new(tx: Transaction<'static, Sqlite>, caller: Caller) -> Self {
        Self { tx, caller }
    }

    pub fn conn(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    /// The person whose capabilities apply: the signed-in person or the API key's owner.
    pub fn person(&self) -> Result<UserId, ApiError> {
        acting_user(&self.caller)
    }

    pub async fn site_scope(
        &mut self,
        site: &SiteId,
        capability: Capability,
    ) -> Result<SiteScope, ApiError> {
        let scope = scope::resolve_for(&mut self.tx, &self.caller, site).await?;
        scope.require(capability)?;
        Ok(scope)
    }

    /// [`QueryCtx::site_scope`] on the state's site, then, for a share viewer, the share's
    /// limits on the state and its panels (`share_scope_exceeded`).
    pub async fn state_scope(
        &mut self,
        state: &ExplorationState,
        panels: &[Panel],
        capability: Capability,
    ) -> Result<SiteScope, ApiError> {
        let scope = self.site_scope(&state.site, capability).await?;
        if let Some(share) = scope.share() {
            sharing::admit_exploration(&mut self.tx, share, state, panels).await?;
        }
        Ok(scope)
    }

    pub async fn workspace_scope(
        &mut self,
        workspace: &WorkspaceId,
        capability: WorkspaceCapability,
    ) -> Result<WorkspaceScope, ApiError> {
        let scope = scope::resolve_workspace_for(&mut self.tx, &self.caller, workspace).await?;
        scope.require(capability)?;
        Ok(scope)
    }

    /// The query counterpart of [`CommandCtx::sources_scope`].
    pub async fn sources_scope(
        &mut self,
        workspace: &WorkspaceId,
        capability: Capability,
    ) -> Result<WorkspaceScope, ApiError> {
        let scope = scope::resolve_workspace_for(&mut self.tx, &self.caller, workspace).await?;
        scope.require_source(capability)?;
        Ok(scope)
    }

    pub async fn require_install_admin(&mut self) -> Result<UserId, ApiError> {
        scope::require_install_admin(&mut self.tx, &self.caller).await
    }

    pub(crate) fn into_tx(self) -> Transaction<'static, Sqlite> {
        self.tx
    }
}

/// A share viewer reaching a handler that is not share-aware is outside the share: `not_found`.
fn acting_user(caller: &Caller) -> Result<UserId, ApiError> {
    match caller {
        Caller::Share { .. } => Err(ApiError::not_found()),
        _ => caller
            .acting_user()
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated)),
    }
}
