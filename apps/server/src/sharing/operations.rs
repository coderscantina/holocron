//! `sharing.*` operations: creating, changing, revoking, listing and resolving shares.
//!
//! Managing a share needs `site.share` on an exploration share's site, or `workspace.write` on a
//! board share's workspace (plus `site.share` on every listed site to create or change it).
//! Grants never exceed the creator's own: `export` needs `site.export` and `recordings` needs
//! `site.recordings.read` on every site the share reaches (`grant_exceeds_principal`).

use holocron_protocol::access::{Capability, WorkspaceCapability};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::exploration::ExplorationState;
use holocron_protocol::ids::{BoardId, ExplorationId, ShareId, UserId, WorkspaceId};
use holocron_protocol::limits::{
    EXPLORATION_RELATIVE_DAYS_MAX, LABEL_MAX_CHARS, SHARE_LIFETIME_MAX_DAYS,
};
use holocron_protocol::operations::sharing::{
    ExplorationLimits, Share, ShareArgs, ShareCreated, ShareList, ShareResult, ShareTarget,
    SharedSite, SharedView, SharingCreateArgs, SharingHandlers, SharingListArgs, SharingUpdateArgs,
};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::limits::{admit_state, state_fields};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::explorations::checked_state;
use crate::identity;
use crate::sites::policy::session_policies;
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;

impl SharingHandlers for AppState {
    async fn sharing_create(
        &self,
        ctx: &mut CommandCtx,
        args: SharingCreateArgs,
    ) -> Result<ShareCreated, ApiError> {
        let authority = manage(ctx, &args.target, true, None).await?;
        authority.grants(args.export, args.recordings)?;
        let name = share_name(&args.name)?;
        check_expiry(args.expires_at, ctx.now())?;
        check_target(ctx, &args.target, args.recordings).await?;

        let id = ShareId::new(crate::db::new_id());
        let (token, digest) = identity::new_session_token();
        let (site, board) = match &args.target {
            ShareTarget::Exploration { state, .. } => (Some(state.site.as_str()), None),
            ShareTarget::Board { board, .. } => (None, Some(board.as_str())),
        };
        let person = ctx.person()?;
        let now = ctx.now();
        sqlx::query(
            "INSERT INTO shares (id, workspace_id, site_id, board_id, name, target, export, recordings,
                                 prefix, token_hash, created_by, created_at, updated_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(authority.workspace.as_str())
        .bind(site)
        .bind(board)
        .bind(&name)
        .bind(serde_json::to_string(&args.target).map_err(ApiError::internal)?)
        .bind(args.export)
        .bind(args.recordings)
        .bind(&token[..8])
        .bind(digest)
        .bind(person.as_str())
        .bind(now)
        .bind(now)
        .bind(args.expires_at)
        .execute(ctx.conn())
        .await?;
        ctx.record(&authority.topic, "share.created", json!({ "id": id }))
            .await?;
        Ok(ShareCreated {
            share: load(ctx.conn(), &id)
                .await?
                .ok_or_else(ApiError::not_found)?,
            secret: Some(token),
            secret_withheld: false,
        })
    }

    async fn sharing_update(
        &self,
        ctx: &mut CommandCtx,
        args: SharingUpdateArgs,
    ) -> Result<ShareResult, ApiError> {
        let share = load(ctx.conn(), &args.share)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let authority = manage(ctx, &share.target, true, args.expected_revision).await?;
        authority.grants(args.export, args.recordings)?;
        if share.revoked_at.is_some() {
            return Err(FieldErrors::one(
                "share",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let name = share_name(&args.name)?;
        check_expiry(args.expires_at, ctx.now())?;
        let target = match (share.target, args.limits) {
            (target, None) => target,
            (
                ShareTarget::Exploration {
                    state, exploration, ..
                },
                Some(limits),
            ) => ShareTarget::Exploration {
                state,
                exploration,
                limits,
            },
            (ShareTarget::Board { .. }, Some(_)) => {
                return Err(FieldErrors::one(
                    "limits",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        };
        check_target(ctx, &target, args.recordings).await?;
        sqlx::query(
            "UPDATE shares SET name = ?, target = ?, export = ?, recordings = ?, expires_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&name)
        .bind(serde_json::to_string(&target).map_err(ApiError::internal)?)
        .bind(args.export)
        .bind(args.recordings)
        .bind(args.expires_at)
        .bind(ctx.now())
        .bind(args.share.as_str())
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &authority.topic,
            "share.updated",
            json!({ "id": args.share }),
        )
        .await?;
        let share = load(ctx.conn(), &args.share)
            .await?
            .ok_or_else(ApiError::not_found)?;
        Ok(ShareResult { share })
    }

    async fn sharing_revoke(
        &self,
        ctx: &mut CommandCtx,
        args: ShareArgs,
    ) -> Result<ShareResult, ApiError> {
        let share = load(ctx.conn(), &args.share)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let authority = manage(ctx, &share.target, false, None).await?;
        if share.revoked_at.is_none() {
            sqlx::query("UPDATE shares SET revoked_at = ?, updated_at = ? WHERE id = ?")
                .bind(ctx.now())
                .bind(ctx.now())
                .bind(args.share.as_str())
                .execute(ctx.conn())
                .await?;
            ctx.record(
                &authority.topic,
                "share.revoked",
                json!({ "id": args.share }),
            )
            .await?;
        }
        let share = load(ctx.conn(), &args.share)
            .await?
            .ok_or_else(ApiError::not_found)?;
        Ok(ShareResult { share })
    }

    async fn sharing_list(
        &self,
        ctx: &mut QueryCtx,
        args: SharingListArgs,
    ) -> Result<Snapshot<ShareList>, ApiError> {
        let (topic, rows): (Topic, Vec<ShareRow>) = match (&args.site, &args.board) {
            (Some(site), None) => {
                let scope = ctx.site_scope(site, Capability::SiteShare).await?;
                let rows = sqlx::query_as(concat!(
                    select_share!(),
                    " WHERE s.site_id = ? ORDER BY s.created_at DESC, s.id"
                ))
                .bind(site.as_str())
                .fetch_all(ctx.conn())
                .await?;
                (Topic::site(scope.site()), rows)
            }
            (None, Some(board)) => {
                let workspace = board_workspace(ctx.conn(), board).await?;
                ctx.workspace_scope(&workspace, WorkspaceCapability::Write)
                    .await?;
                let rows = sqlx::query_as(concat!(
                    select_share!(),
                    " WHERE s.board_id = ? ORDER BY s.created_at DESC, s.id"
                ))
                .bind(board.as_str())
                .fetch_all(ctx.conn())
                .await?;
                (Topic::workspace(&workspace), rows)
            }
            _ => return Err(FieldErrors::one("site", FieldErrorCode::InvalidValue, None)),
        };
        Ok(Snapshot {
            topic,
            data: ShareList {
                shares: rows.into_iter().map(share).collect::<Result<_, _>>()?,
            },
        })
    }

    async fn sharing_resolve(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<SharedView>, ApiError> {
        let viewer = ctx
            .caller()
            .share()
            .cloned()
            .ok_or_else(ApiError::not_found)?;
        let (name,): (String,) = sqlx::query_as("SELECT name FROM shares WHERE id = ?")
            .bind(viewer.id.as_str())
            .fetch_one(ctx.conn())
            .await?;
        let (topic, reached) = match &viewer.target {
            ShareTarget::Exploration { state, .. } => {
                (Topic::site(&state.site), vec![state.site.clone()])
            }
            ShareTarget::Board { sites, .. } => {
                (Topic::workspace(&viewer.workspace), sites.clone())
            }
        };
        let mut sites = Vec::new();
        for site in reached {
            let row: Option<(String, String)> =
                sqlx::query_as("SELECT name, domain FROM sites WHERE id = ? AND workspace_id = ?")
                    .bind(site.as_str())
                    .bind(viewer.workspace.as_str())
                    .fetch_optional(ctx.conn())
                    .await?;
            let Some((name, domain)) = row else { continue };
            let policies = session_policies(ctx.conn(), site.as_str())
                .await?
                .ok_or_else(|| ApiError::internal(format!("site {site} has no session policy")))?;
            sites.push(SharedSite {
                timezone: policies.current().policy.timezone().name().to_owned(),
                id: site,
                name,
                domain,
            });
        }
        Ok(Snapshot {
            topic,
            data: SharedView {
                name,
                target: viewer.target.clone(),
                export: viewer.export,
                recordings: viewer.recordings,
                expires_at: viewer.expires_at,
                sites,
            },
        })
    }
}

/// What the caller may grant on a share's sites, its workspace and the topic share events go to.
struct Authority {
    workspace: WorkspaceId,
    topic: Topic,
    export: bool,
    recordings: bool,
}

impl Authority {
    fn grants(&self, export: bool, recordings: bool) -> Result<(), ApiError> {
        if (export && !self.export) || (recordings && !self.recordings) {
            Err(ApiError::new(ErrorCode::GrantExceedsPrincipal))
        } else {
            Ok(())
        }
    }
}

/// Resolves the caller's right to manage shares of `target`. `sites` also requires
/// `site.share` on every site of a board share, as creating and changing one do.
async fn manage(
    ctx: &mut CommandCtx,
    target: &ShareTarget,
    sites: bool,
    expected_revision: Option<i64>,
) -> Result<Authority, ApiError> {
    match target {
        ShareTarget::Exploration { state, .. } => {
            let scope = ctx
                .site_scope(&state.site, Capability::SiteShare, expected_revision)
                .await?;
            Ok(Authority {
                workspace: scope.workspace().clone(),
                topic: Topic::site(scope.site()),
                export: scope.can(Capability::SiteExport),
                recordings: scope.can(Capability::SiteRecordingsRead),
            })
        }
        ShareTarget::Board {
            board,
            sites: listed,
        } => {
            let workspace = board_workspace(ctx.conn(), board).await?;
            ctx.workspace_scope(&workspace, WorkspaceCapability::Write, expected_revision)
                .await?;
            let mut authority = Authority {
                topic: Topic::workspace(&workspace),
                workspace,
                export: true,
                recordings: true,
            };
            if sites {
                for site in listed {
                    let scope = ctx.site_scope(site, Capability::SiteShare, None).await?;
                    if scope.workspace() != &authority.workspace {
                        return Err(FieldErrors::one(
                            "target",
                            FieldErrorCode::InvalidValue,
                            None,
                        ));
                    }
                    authority.export &= scope.can(Capability::SiteExport);
                    authority.recordings &= scope.can(Capability::SiteRecordingsRead);
                }
            }
            Ok(authority)
        }
    }
}

/// An exploration share's state must be valid and its limits no wider than the state reads,
/// while still admitting the state itself. Board shares list their sites once and grant no
/// recordings: without a range limit or pinned filter, a recordings grant would reach every
/// replay of the site.
async fn check_target(
    ctx: &mut CommandCtx,
    target: &ShareTarget,
    recordings: bool,
) -> Result<(), ApiError> {
    let invalid = || FieldErrors::one("target", FieldErrorCode::InvalidValue, None);
    match target {
        ShareTarget::Exploration {
            state,
            exploration,
            limits,
        } => {
            checked_state(state, &state.site)?;
            if let Some(exploration) = exploration {
                own_exploration(ctx, exploration, state).await?;
            }
            if !limits_fit(state, limits) {
                return Err(invalid());
            }
            Ok(())
        }
        ShareTarget::Board { sites, .. } => {
            if recordings {
                return Err(FieldErrors::one(
                    "recordings",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
            let mut unique = sites.clone();
            unique.sort();
            unique.dedup();
            if sites.is_empty() || unique.len() != sites.len() {
                return Err(invalid());
            }
            Ok(())
        }
    }
}

fn limits_fit(state: &ExplorationState, limits: &ExplorationLimits) -> bool {
    let fields = state_fields(state);
    (1..=EXPLORATION_RELATIVE_DAYS_MAX as u16).contains(&limits.range_max_days)
        && limits.measures.iter().all(|m| fields.measures.contains(m))
        && limits
            .dimensions
            .iter()
            .all(|d| fields.dimensions.contains(d))
        && limits
            .properties
            .iter()
            .all(|p| fields.properties.contains(p))
        && admit_state(state, limits, state, &[]).is_ok()
}

/// A saved exploration named by a share must be the caller's own, on the state's site.
async fn own_exploration(
    ctx: &mut CommandCtx,
    exploration: &ExplorationId,
    state: &ExplorationState,
) -> Result<(), ApiError> {
    let person = ctx.person()?;
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM saved_explorations WHERE id = ? AND owner_id = ? AND site_id = ?",
    )
    .bind(exploration.as_str())
    .bind(person.as_str())
    .bind(state.site.as_str())
    .fetch_optional(ctx.conn())
    .await?;
    found.map(drop).ok_or_else(ApiError::not_found)
}

async fn board_workspace(
    conn: &mut SqliteConnection,
    board: &BoardId,
) -> Result<WorkspaceId, ApiError> {
    let workspace: Option<String> =
        sqlx::query_scalar("SELECT workspace_id FROM boards WHERE id = ?")
            .bind(board.as_str())
            .fetch_optional(conn)
            .await?;
    workspace
        .map(WorkspaceId::new)
        .ok_or_else(ApiError::not_found)
}

fn share_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 {
        return Err(FieldErrors::one("name", FieldErrorCode::Required, None));
    }
    if length > LABEL_MAX_CHARS {
        return Err(FieldErrors::one(
            "name",
            FieldErrorCode::TooLong,
            Some(LABEL_MAX_CHARS as u32),
        ));
    }
    Ok(name.to_owned())
}

/// Every share expires, at most 366 days ahead (decision 0012).
fn check_expiry(expires_at: i64, now: i64) -> Result<(), ApiError> {
    let latest = now + i64::from(SHARE_LIFETIME_MAX_DAYS) * DAY_MS;
    if expires_at <= now || expires_at > latest {
        return Err(FieldErrors::one(
            "expires_at",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    Ok(())
}

macro_rules! select_share {
    () => {
        "SELECT s.id, s.name, s.target, s.export, s.recordings, s.expires_at, s.prefix, u.id,
                s.created_at, s.revoked_at
         FROM shares s LEFT JOIN users u ON u.id = s.created_by"
    };
}
use select_share;

type ShareRow = (
    String,
    String,
    String,
    bool,
    bool,
    i64,
    String,
    Option<String>,
    i64,
    Option<i64>,
);

async fn load(conn: &mut SqliteConnection, id: &ShareId) -> Result<Option<Share>, ApiError> {
    let row: Option<ShareRow> = sqlx::query_as(concat!(select_share!(), " WHERE s.id = ?"))
        .bind(id.as_str())
        .fetch_optional(conn)
        .await?;
    row.map(share).transpose()
}

fn share(
    (id, name, target, export, recordings, expires_at, prefix, author, created_at, revoked_at): ShareRow,
) -> Result<Share, ApiError> {
    Ok(Share {
        id: ShareId::new(id),
        name,
        target: serde_json::from_str(&target).map_err(ApiError::internal)?,
        export,
        recordings,
        expires_at,
        prefix,
        created_by: author.map(UserId::new),
        created_at,
        revoked_at,
    })
}
