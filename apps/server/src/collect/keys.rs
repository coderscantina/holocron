//! Site collection keys on `/api/send` and `/api/batch` (decision 0001, Transient inputs and
//! Persistent consent transport). `Authorization: Bearer <secret>` names a key, and its items
//! for the key's site are trusted: `payload.ip` and `payload.userAgent` replace the
//! connection's. Another scheme is not a collection key and leaves the request as it is, so a
//! browser behind HTTP authentication keeps collecting as before.

use axum::http::{HeaderMap, header};
use holocron_protocol::limits::COLLECTION_KEY_SCHEME;
use uuid::Uuid;

use super::Collector;
use crate::identity::token_digest;

/// Who vouches for a request's visitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// No key: the connection is the visitor.
    Connection,
    /// A valid key of this site.
    Site(Uuid),
    /// An empty, unknown or revoked key: the request is refused.
    Refused,
}

impl Collector {
    /// One index probe on the secret's digest, on the read pool.
    pub(super) async fn trust(&self, headers: &HeaderMap) -> anyhow::Result<Trust> {
        let Some(token) = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .and_then(|(scheme, token)| (scheme == COLLECTION_KEY_SCHEME).then(|| token.trim()))
        else {
            return Ok(Trust::Connection);
        };
        if token.is_empty() {
            return Ok(Trust::Refused);
        }
        let site: Option<String> = sqlx::query_scalar(
            "SELECT site_id FROM collection_keys WHERE secret_hash = ? AND revoked_at IS NULL",
        )
        .bind(token_digest(token))
        .fetch_optional(&self.db.reader)
        .await?;
        Ok(match site {
            Some(site) => Trust::Site(Uuid::try_parse(&site)?),
            None => Trust::Refused,
        })
    }
}
