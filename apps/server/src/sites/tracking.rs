//! `sites.trackingCode` and `collectionKeys.*`: putting collection on a site (CAP-COLLECT-01)
//! and the keys of trusted server integrations (decision 0001).
//!
//! A collection key belongs to its site, not to a person, so it outlives its creator's
//! membership and is removed with the site. Its secret is shown once and stored as a SHA-256
//! digest, like API keys (0012); `/api/send` and `/api/batch` look it up per request, so a
//! revocation applies to the next request.

use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::{CollectionKeyId, SiteId};
use holocron_protocol::limits::{COLLECTION_KEYS_PER_SITE_MAX, LABEL_MAX_CHARS};
use holocron_protocol::operations::sites::SitesGetArgs;
use holocron_protocol::operations::tracking::*;
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;
use url::Url;

use crate::context::{CommandCtx, QueryCtx};
use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};
use crate::identity::new_session_token;
use crate::recordings::policy as recording_policy;
use crate::state::AppState;

/// Characters of the secret kept in clear to tell keys apart.
const PREFIX_CHARS: usize = 8;

impl TrackingHandlers for AppState {
    async fn sites_tracking_code(
        &self,
        ctx: &mut QueryCtx,
        args: SitesTrackingCodeArgs,
    ) -> Result<Snapshot<TrackingCode>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        // A configured public URL wins; else the dashboard's origin, as upstream uses the page's.
        let origin = match (&self.config.public_origin, args.origin.as_deref()) {
            (Some(origin), _) => origin.clone(),
            (None, Some(given)) => browser_origin(given)?,
            (None, None) => String::new(),
        };
        let site = scope.site();
        let tag = |path: String| ScriptTag {
            code: format!(
                r#"<script defer src="{}" data-website-id="{site}"></script>"#,
                attribute(&format!("{origin}{path}"))
            ),
            path,
        };
        let tracker = self
            .config
            .collect
            .tracker_script_names
            .iter()
            .map(|name| format!("/{name}"))
            .chain(["/script.js".to_owned()])
            .map(tag)
            .collect();
        let policy = recording_policy::load(ctx.conn(), site.as_str()).await?;
        let records = policy.replay_enabled || policy.heatmap_enabled;
        Ok(Snapshot {
            topic: Topic::site(site),
            data: TrackingCode {
                tracker,
                recorder: records.then(|| tag("/recorder.js".to_owned())),
            },
        })
    }

    async fn collection_keys_list(
        &self,
        ctx: &mut QueryCtx,
        args: SitesGetArgs,
    ) -> Result<Snapshot<CollectionKeyList>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: CollectionKeyList {
                keys: list(ctx.conn(), scope.site(), None).await?,
            },
        })
    }

    async fn collection_keys_create(
        &self,
        ctx: &mut CommandCtx,
        args: CollectionKeysCreateArgs,
    ) -> Result<CollectionKeyCreated, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure, None)
            .await?;
        let site = scope.site();
        let name = args.name.trim().to_owned();
        match name.chars().count() {
            0 => return Err(FieldErrors::one("name", FieldErrorCode::Required, None)),
            n if n > LABEL_MAX_CHARS => {
                return Err(FieldErrors::one(
                    "name",
                    FieldErrorCode::TooLong,
                    Some(LABEL_MAX_CHARS as u32),
                ));
            }
            _ => {}
        }
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM collection_keys WHERE site_id = ? AND revoked_at IS NULL",
        )
        .bind(site.as_str())
        .fetch_one(ctx.conn())
        .await?;
        if active as usize >= COLLECTION_KEYS_PER_SITE_MAX {
            return Err(ApiError::with_message(
                ErrorCode::InvalidArguments,
                format!(
                    "A site can hold at most {COLLECTION_KEYS_PER_SITE_MAX} active collection keys."
                ),
            ));
        }

        let (secret, digest) = new_session_token();
        let key = CollectionKey {
            id: CollectionKeyId::new(new_id()),
            site: site.clone(),
            name,
            prefix: secret[..PREFIX_CHARS].to_owned(),
            created_at: ctx.now(),
            revoked_at: None,
        };
        sqlx::query(
            "INSERT INTO collection_keys (id, site_id, name, prefix, secret_hash, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(key.id.as_str())
        .bind(site.as_str())
        .bind(&key.name)
        .bind(&key.prefix)
        .bind(&digest)
        .bind(key.created_at)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::site(site),
            "collection_key.created",
            json!({ "key": key.id }),
        )
        .await?;
        Ok(CollectionKeyCreated {
            key,
            secret: Some(secret),
            secret_withheld: false,
        })
    }

    async fn collection_keys_revoke(
        &self,
        ctx: &mut CommandCtx,
        args: CollectionKeyArgs,
    ) -> Result<CollectionKeyResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure, None)
            .await?;
        let site = scope.site();
        let revoked = sqlx::query(
            "UPDATE collection_keys SET revoked_at = ?
             WHERE id = ? AND site_id = ? AND revoked_at IS NULL",
        )
        .bind(ctx.now())
        .bind(args.key.as_str())
        .bind(site.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        let key = list(ctx.conn(), site, Some(&args.key))
            .await?
            .pop()
            .ok_or_else(ApiError::not_found)?;
        if revoked > 0 {
            ctx.record(
                &Topic::site(site),
                "collection_key.revoked",
                json!({ "key": key.id }),
            )
            .await?;
        }
        Ok(CollectionKeyResult { key })
    }
}

/// `scheme://host[:port]` of a dashboard URL; anything but http and https is refused.
fn browser_origin(given: &str) -> Result<String, ApiError> {
    Url::parse(given.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.origin().ascii_serialization())
        .ok_or_else(|| FieldErrors::one("origin", FieldErrorCode::InvalidValue, None))
}

/// Escapes text for a double-quoted HTML attribute.
fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The site's keys, newest first, or the one key `only`.
async fn list(
    conn: &mut SqliteConnection,
    site: &SiteId,
    only: Option<&CollectionKeyId>,
) -> Result<Vec<CollectionKey>, sqlx::Error> {
    let rows: Vec<(String, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, name, prefix, created_at, revoked_at FROM collection_keys
         WHERE site_id = ?1 AND (?2 IS NULL OR id = ?2) ORDER BY created_at DESC, id",
    )
    .bind(site.as_str())
    .bind(only.map(CollectionKeyId::as_str))
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, prefix, created_at, revoked_at)| CollectionKey {
            id: CollectionKeyId::new(id),
            site: site.clone(),
            name,
            prefix,
            created_at,
            revoked_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_origins_are_normalized_and_other_schemes_refused() {
        assert_eq!(
            browser_origin("https://Stats.Example.com:443/settings").unwrap(),
            "https://stats.example.com"
        );
        assert_eq!(
            browser_origin("http://localhost:5173").unwrap(),
            "http://localhost:5173"
        );
        assert!(browser_origin("javascript:alert(1)").is_err());
    }
}
