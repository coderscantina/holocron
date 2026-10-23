//! Who is calling, resolved once per request, and the credential-level part of the decision
//! order in `fixtures/access/README.md` (steps 1 to 3).
//!
//! Transport (decision 0012): a person sends the session cookie; an API key sends
//! `Authorization: Bearer <secret>`; a share viewer sends `Authorization: Share <token>`. An
//! `Authorization` header wins over the cookie, so a signed-in browser opening a share link
//! reads as the share, never as the person.

use std::sync::Arc;

use axum::http::{HeaderMap, header};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::ids::{ApiKeyId, SiteId, UserId};
use holocron_protocol::limits::{
    API_KEY_SCHEME, REAUTHENTICATION_WINDOW_MS, SESSION_IDLE_MS, SHARE_SCHEME,
};
use holocron_protocol::operations::api_keys::KeySites;
use holocron_protocol::registry::{Access, OperationInfo, ShareReach};
use sqlx::SqliteConnection;

use super::{CHALLENGE_COOKIE, SESSION_COOKIE, token_digest};
use crate::db::{Db, now_ms};
use crate::error::ApiError;
use crate::sharing::{self, ShareViewer};

/// `last_seen_at` and `last_used_at` are written at most this often (decision 0012).
const TOUCH_EVERY_MS: i64 = 60_000;

/// The principal behind one request.
#[derive(Debug, Clone)]
pub enum Caller {
    Anonymous {
        address: String,
        /// Digest of the login challenge cookie between password and second factor.
        challenge: Option<String>,
    },
    Person {
        user: UserId,
        /// Digest of the session cookie, the session's ID.
        session: String,
        address: String,
        /// Must enroll a second factor first (decision 0007). Derived on every request.
        enrollment_only: bool,
        reauthenticated_at: Option<i64>,
    },
    Key(Arc<KeyCaller>),
    Share {
        viewer: Arc<ShareViewer>,
        address: String,
    },
}

/// An API key request. The key acts as `owner` with the owner's current capabilities, limited
/// to `operations` and `sites`.
#[derive(Debug)]
pub struct KeyCaller {
    pub id: ApiKeyId,
    pub owner: UserId,
    pub operations: Vec<String>,
    pub sites: KeySites,
    pub address: String,
}

impl KeyCaller {
    pub fn reaches(&self, site: &SiteId) -> bool {
        match &self.sites {
            KeySites::All => true,
            KeySites::Only { sites } => sites.contains(site),
        }
    }
}

impl Caller {
    pub fn address(&self) -> &str {
        match self {
            Caller::Anonymous { address, .. }
            | Caller::Person { address, .. }
            | Caller::Share { address, .. } => address,
            Caller::Key(key) => &key.address,
        }
    }

    /// The signed-in person. `None` for API keys, share viewers and anonymous callers.
    pub fn user(&self) -> Option<&UserId> {
        match self {
            Caller::Person { user, .. } => Some(user),
            _ => None,
        }
    }

    /// The person whose capabilities apply: the signed-in person or the key's owner.
    pub fn acting_user(&self) -> Option<&UserId> {
        match self {
            Caller::Person { user, .. } => Some(user),
            Caller::Key(key) => Some(&key.owner),
            _ => None,
        }
    }

    pub fn share(&self) -> Option<&ShareViewer> {
        match self {
            Caller::Share { viewer, .. } => Some(viewer),
            _ => None,
        }
    }

    /// Owner of the caller's idempotency keys. A key's commands are its own, apart from its
    /// owner's browser sessions.
    pub fn key_owner(&self) -> String {
        match self {
            Caller::Person { user, .. } => format!("user:{user}"),
            Caller::Key(key) => format!("key:{}", key.id),
            Caller::Share { viewer, .. } => format!("share:{}", viewer.id),
            Caller::Anonymous { address, .. } => format!("addr:{address}"),
        }
    }

    /// Steps 1 to 3 of the decision order for `op`, before any resource is looked up:
    /// no credential, key grants, share reach, and the enrollment-only session (0007).
    pub fn admit(&self, op: &OperationInfo) -> Result<(), ApiError> {
        match self {
            Caller::Anonymous { .. } if op.access == Access::Person => {
                Err(ApiError::new(ErrorCode::Unauthenticated))
            }
            Caller::Anonymous { .. } => Ok(()),
            Caller::Key(key) => {
                if op.policy.key_grantable && key.operations.iter().any(|name| name == op.name) {
                    Ok(())
                } else {
                    Err(ApiError::new(ErrorCode::OperationNotGranted))
                }
            }
            Caller::Share { viewer, .. } => {
                let reached = match op.policy.share {
                    ShareReach::None => op.access == Access::Anonymous,
                    ShareReach::Read => true,
                    ShareReach::Export => viewer.export,
                    ShareReach::Recordings => viewer.recordings,
                };
                if reached {
                    Ok(())
                } else {
                    Err(ApiError::not_found())
                }
            }
            Caller::Person {
                enrollment_only: true,
                ..
            } if op.access == Access::Person && !op.policy.enrollment => {
                Err(ApiError::new(ErrorCode::SecondFactorEnrollmentRequired))
            }
            Caller::Person { .. } => Ok(()),
        }
    }

    /// Step 6: a passed password check within the last 10 minutes of this login session.
    pub fn require_fresh_login(&self, now: i64) -> Result<(), ApiError> {
        match self {
            Caller::Person {
                reauthenticated_at: Some(at),
                ..
            } if now - at < REAUTHENTICATION_WINDOW_MS => Ok(()),
            _ => Err(ApiError::new(ErrorCode::ReauthenticationRequired)),
        }
    }
}

/// Resolves the request's credential. No credential is an anonymous caller. A bad API key is
/// `unauthenticated` and a dead share token `not_found` for every operation, so revoked looks
/// like unknown (R-KEY-REVOKED, R-SHARE-LIFE). A missing, unknown, expired or idle session cookie
/// is an anonymous caller.
pub async fn resolve_caller(
    db: &Db,
    headers: &HeaderMap,
    address: String,
) -> Result<Caller, ApiError> {
    let mut conn = db.reader.acquire().await?;
    let now = now_ms();
    match authorization(headers) {
        Some(Credential::Key(secret)) => {
            let key = key(db, &mut conn, &token_digest(secret), address, now)
                .await?
                .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
            Ok(Caller::Key(Arc::new(key)))
        }
        Some(Credential::Share(token)) => {
            let viewer = sharing::viewer(&mut conn, &token_digest(token), now)
                .await?
                .ok_or_else(ApiError::not_found)?;
            Ok(Caller::Share {
                viewer: Arc::new(viewer),
                address,
            })
        }
        Some(Credential::Unknown) => Err(ApiError::new(ErrorCode::Unauthenticated)),
        None => person(db, &mut conn, headers, address, now).await,
    }
}

enum Credential<'a> {
    Key(&'a str),
    Share(&'a str),
    Unknown,
}

fn authorization(headers: &HeaderMap) -> Option<Credential<'_>> {
    let value = headers.get(header::AUTHORIZATION)?;
    let Some((scheme, token)) = value.to_str().ok().and_then(|v| v.split_once(' ')) else {
        return Some(Credential::Unknown);
    };
    let token = token.trim();
    Some(match scheme {
        _ if token.is_empty() => Credential::Unknown,
        API_KEY_SCHEME => Credential::Key(token),
        SHARE_SCHEME => Credential::Share(token),
        _ => Credential::Unknown,
    })
}

/// Whether the request authenticates with an API key. Such commands skip the Origin check.
pub fn has_api_key(headers: &HeaderMap) -> bool {
    matches!(authorization(headers), Some(Credential::Key(_)))
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(cookie, _)| *cookie == name)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

async fn person(
    db: &Db,
    conn: &mut SqliteConnection,
    headers: &HeaderMap,
    address: String,
    now: i64,
) -> Result<Caller, ApiError> {
    let challenge = cookie(headers, CHALLENGE_COOKIE).map(token_digest);
    let Some(token) = cookie(headers, SESSION_COOKIE) else {
        return Ok(Caller::Anonymous { address, challenge });
    };
    let session = token_digest(token);
    // A person must use 2FA when the installation, their account or any of their teams
    // requires it; without a confirmed factor the session is enrollment-only (0007).
    let row: Option<(String, Option<i64>, i64, bool)> = sqlx::query_as(concat!(
        "SELECT s.user_id, s.reauthenticated_at, s.last_seen_at,
                NOT EXISTS (SELECT 1 FROM two_factor t WHERE t.user_id = s.user_id AND t.secret IS NOT NULL)
                AND ",
        two_factor_required_sql!("s.user_id"),
        " FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.id = ? AND s.expires_at > ? AND s.last_seen_at > ?"
    ))
    .bind(&session)
    .bind(now)
    .bind(now - SESSION_IDLE_MS)
    .fetch_optional(conn)
    .await?;
    let Some((user, reauthenticated_at, last_seen_at, enrollment_only)) = row else {
        return Ok(Caller::Anonymous { address, challenge });
    };
    if now - last_seen_at >= TOUCH_EVERY_MS {
        touch(
            db,
            "UPDATE sessions SET last_seen_at = ? WHERE id = ?",
            session.clone(),
            now,
        );
    }
    Ok(Caller::Person {
        user: UserId::new(user),
        session,
        address,
        enrollment_only,
        reauthenticated_at,
    })
}

/// Records use of a session or key off the request path. Reads stay off the single writer;
/// at most one write a minute per credential.
fn touch(db: &Db, sql: &'static str, id: String, now: i64) {
    let writer = db.writer.clone();
    tokio::spawn(async move {
        if let Err(error) = sqlx::query(sql).bind(now).bind(id).execute(&writer).await {
            tracing::warn!(%error, "could not record credential use");
        }
    });
}

/// The active key with this secret digest, with its grants.
async fn key(
    db: &Db,
    conn: &mut SqliteConnection,
    secret_hash: &str,
    address: String,
    now: i64,
) -> Result<Option<KeyCaller>, ApiError> {
    let row: Option<(String, String, bool, Option<i64>)> = sqlx::query_as(
        "SELECT id, owner_id, all_sites, last_used_at FROM api_keys
         WHERE secret_hash = ? AND revoked_at IS NULL",
    )
    .bind(secret_hash)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, owner, all_sites, last_used_at)) = row else {
        return Ok(None);
    };
    if last_used_at.is_none_or(|at| now - at >= TOUCH_EVERY_MS) {
        touch(
            db,
            "UPDATE api_keys SET last_used_at = ? WHERE id = ?",
            id.clone(),
            now,
        );
    }
    let operations: Vec<String> =
        sqlx::query_scalar("SELECT operation FROM api_key_operations WHERE key_id = ?")
            .bind(&id)
            .fetch_all(&mut *conn)
            .await?;
    let sites = if all_sites {
        KeySites::All
    } else {
        let sites: Vec<String> =
            sqlx::query_scalar("SELECT site_id FROM api_key_sites WHERE key_id = ?")
                .bind(&id)
                .fetch_all(&mut *conn)
                .await?;
        KeySites::Only {
            sites: sites.into_iter().map(SiteId::new).collect(),
        }
    };
    Ok(Some(KeyCaller {
        id: ApiKeyId::new(id),
        owner: UserId::new(owner),
        operations,
        sites,
        address,
    }))
}
