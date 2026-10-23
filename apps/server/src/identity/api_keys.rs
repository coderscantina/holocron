//! `apiKeys.*` operations: scoped keys of the caller.
//!
//! A key never holds more than its owner: at creation every operation must be key-grantable and
//! covered by the owner's role on every listed site (in some workspace for `all`), and every
//! request resolves the owner's current role again (R-KEY-CEILING).

use std::collections::{BTreeSet, HashMap};

use holocron_protocol::access::{Capability, Role, WorkspaceCapability};
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::{ApiKeyId, SiteId, UserId};
use holocron_protocol::limits::{API_KEYS_PER_USER_MAX, LABEL_MAX_CHARS};
use holocron_protocol::operations::{self, api_keys::*};
use holocron_protocol::registry::Snapshot;
use serde_json::json;
use sqlx::SqliteConnection;

use super::{new_session_token, signed_in};
use crate::context::{CommandCtx, QueryCtx};
use crate::db::new_id;
use crate::error::{ApiError, FieldErrors};
use crate::sites::scope;
use crate::state::AppState;

/// Characters of the secret kept in clear to tell keys apart.
const PREFIX_CHARS: usize = 8;

impl ApiKeysHandlers for AppState {
    async fn api_keys_create(
        &self,
        ctx: &mut CommandCtx,
        args: ApiKeysCreateArgs,
    ) -> Result<ApiKeyCreated, ApiError> {
        let user = signed_in(ctx.caller())?;
        let granted: BTreeSet<String> = args.operations.into_iter().collect();
        let sites = match args.sites {
            KeySites::All => KeySites::All,
            KeySites::Only { sites } => KeySites::Only {
                sites: sites.into_iter().fold(Vec::new(), |mut unique, site| {
                    if !unique.contains(&site) {
                        unique.push(site);
                    }
                    unique
                }),
            },
        };
        let held = held_capabilities(ctx.conn(), &user, &sites).await?;
        let mut unknown = false;
        for name in &granted {
            match operations::all().find(|op| op.name == name) {
                None => unknown = true,
                Some(op) if !op.policy.key_grantable => {
                    return Err(ApiError::new(ErrorCode::GrantExceedsPrincipal));
                }
                Some(_) => {
                    let exceeds = match key_need(name) {
                        None => true,
                        // Workspace resources are reachable by `all`-site keys only (0012).
                        Some(Need::Workspace(_)) if sites != KeySites::All => true,
                        Some(need) => held.as_ref().is_some_and(|held| !held.contains(&need)),
                    };
                    if exceeds {
                        return Err(ApiError::new(ErrorCode::GrantExceedsPrincipal));
                    }
                }
            }
        }
        ctx.require_fresh_login()?;

        let name = args.name.trim().to_owned();
        let mut errors = FieldErrors::default();
        if unknown {
            errors.add("operations", FieldErrorCode::InvalidValue, None);
        } else if granted.is_empty() {
            errors.add("operations", FieldErrorCode::Required, None);
        }
        match name.chars().count() {
            0 => errors.add("name", FieldErrorCode::Required, None),
            n if n > LABEL_MAX_CHARS => {
                errors.add(
                    "name",
                    FieldErrorCode::TooLong,
                    Some(LABEL_MAX_CHARS as u32),
                );
            }
            _ => {}
        }
        errors.finish()?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE owner_id = ? AND revoked_at IS NULL",
        )
        .bind(user.as_str())
        .fetch_one(ctx.conn())
        .await?;
        if active as usize >= API_KEYS_PER_USER_MAX {
            return Err(ApiError::with_message(
                ErrorCode::InvalidArguments,
                format!("A person can hold at most {API_KEYS_PER_USER_MAX} active keys."),
            ));
        }

        let (secret, digest) = new_session_token();
        let key = ApiKey {
            id: ApiKeyId::new(new_id()),
            name,
            prefix: secret[..PREFIX_CHARS].to_owned(),
            operations: granted.into_iter().collect(),
            sites,
            created_at: ctx.now(),
            last_used_at: None,
            revoked_at: None,
        };
        insert(ctx.conn(), &user, &key, &digest).await?;
        ctx.record(
            &Topic::user(&user),
            "api_key.created",
            json!({ "key": key.id }),
        )
        .await?;
        Ok(ApiKeyCreated {
            key,
            secret: Some(secret),
            secret_withheld: false,
        })
    }

    async fn api_keys_revoke(
        &self,
        ctx: &mut CommandCtx,
        args: ApiKeyArgs,
    ) -> Result<ApiKeyResult, ApiError> {
        let user = signed_in(ctx.caller())?;
        let revoked = sqlx::query(
            "UPDATE api_keys SET revoked_at = ? WHERE id = ? AND owner_id = ? AND revoked_at IS NULL",
        )
        .bind(ctx.now())
        .bind(args.key.as_str())
        .bind(user.as_str())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        let key = list(ctx.conn(), &user, Some(&args.key))
            .await?
            .pop()
            .ok_or_else(ApiError::not_found)?;
        if revoked > 0 {
            ctx.record(
                &Topic::user(&user),
                "api_key.revoked",
                json!({ "key": key.id }),
            )
            .await?;
        }
        Ok(ApiKeyResult { key })
    }

    async fn api_keys_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<ApiKeyList>, ApiError> {
        let user = signed_in(ctx.caller())?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: ApiKeyList {
                keys: list(ctx.conn(), &user, None).await?,
            },
        })
    }
}

/// What an operation needs on a site or on the site's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Need {
    Site(Capability),
    Workspace(WorkspaceCapability),
}

/// The capability every key-grantable operation needs. A test keeps it complete; an operation
/// missing here cannot be granted.
fn key_need(name: &str) -> Option<Need> {
    use Capability::*;
    use WorkspaceCapability::{Read, Write};
    Some(match name {
        "analytics.explore"
        | "analytics.freshness"
        | "fields.catalog"
        | "fields.values"
        | "realtime.snapshot"
        | "sessions.get"
        | "sessions.timeline"
        | "explorations.list"
        | "explorations.get"
        | "explorations.save"
        | "explorations.delete"
        | "segments.list"
        | "annotations.list"
        | "goals.list"
        | "goals.get"
        | "summaries.forSites"
        | "summaries.findings"
        | "sites.get" => Need::Site(SiteRead),
        "analytics.export" => Need::Site(SiteExport),
        "recordings.list" | "recordings.get" | "recordings.save" | "recordings.unsave"
        | "heatmaps.pages" | "heatmaps.cells" => Need::Site(SiteRecordingsRead),
        "segments.save" | "segments.delete" | "annotations.create" | "annotations.update"
        | "annotations.delete" | "goals.save" | "goals.archive" => Need::Site(SiteDefinitionsWrite),
        "sites.update"
        | "sites.updateSessionPolicy"
        | "sites.updateIdentityPolicy"
        | "sites.identityPolicy"
        | "recordings.policy"
        | "recordings.updatePolicy"
        | "recordings.budget"
        | "recalculations.start"
        | "recalculations.status" => Need::Site(SiteConfigure),
        // Links and pixels have the site capabilities of the workspace role (decision 0020).
        "links.list" | "links.explore" | "pixels.list" | "pixels.explore" => Need::Site(SiteRead),
        "links.update" | "pixels.update" => Need::Site(SiteConfigure),
        "links.create" | "pixels.create" => Need::Workspace(Write),
        "boards.list" | "boards.get" => Need::Workspace(Read),
        "boards.create" | "boards.update" | "boards.pin" | "boards.delete" => {
            Need::Workspace(Write)
        }
        _ => return None,
    })
}

/// What the owner holds on every listed site, or in any workspace for `all`. `None` for a key
/// without sites, which reaches nothing and so exceeds nobody. A listed site the owner cannot
/// see is `not_found`, like a missing one.
async fn held_capabilities(
    conn: &mut SqliteConnection,
    user: &UserId,
    sites: &KeySites,
) -> Result<Option<BTreeSet<Need>>, ApiError> {
    match sites {
        KeySites::All => {
            let roles: Vec<String> =
                sqlx::query_scalar("SELECT role FROM memberships WHERE user_id = ?")
                    .bind(user.as_str())
                    .fetch_all(conn)
                    .await?;
            Ok(Some(
                roles
                    .iter()
                    .filter_map(|role| Role::parse(role))
                    .flat_map(|role| {
                        let site = role.site_capabilities().iter().copied().map(Need::Site);
                        let workspace = role.workspace_capabilities().iter().copied();
                        site.chain(workspace.map(Need::Workspace))
                    })
                    .collect(),
            ))
        }
        KeySites::Only { sites } => {
            let mut held: Option<BTreeSet<Need>> = None;
            for site in sites {
                let scope = scope::resolve(&mut *conn, user, site).await?;
                let capabilities: BTreeSet<Need> =
                    scope.capabilities().into_iter().map(Need::Site).collect();
                held = Some(match held {
                    None => capabilities,
                    Some(held) => held.intersection(&capabilities).copied().collect(),
                });
            }
            Ok(held)
        }
    }
}

async fn insert(
    conn: &mut SqliteConnection,
    owner: &UserId,
    key: &ApiKey,
    digest: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO api_keys (id, owner_id, name, prefix, secret_hash, all_sites, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(key.id.as_str())
    .bind(owner.as_str())
    .bind(&key.name)
    .bind(&key.prefix)
    .bind(digest)
    .bind(key.sites == KeySites::All)
    .bind(key.created_at)
    .execute(&mut *conn)
    .await?;
    for operation in &key.operations {
        sqlx::query("INSERT INTO api_key_operations (key_id, operation) VALUES (?, ?)")
            .bind(key.id.as_str())
            .bind(operation)
            .execute(&mut *conn)
            .await?;
    }
    if let KeySites::Only { sites } = &key.sites {
        for site in sites {
            sqlx::query("INSERT INTO api_key_sites (key_id, site_id) VALUES (?, ?)")
                .bind(key.id.as_str())
                .bind(site.as_str())
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

/// The owner's keys, newest first, or the one key `only`.
async fn list(
    conn: &mut SqliteConnection,
    owner: &UserId,
    only: Option<&ApiKeyId>,
) -> Result<Vec<ApiKey>, sqlx::Error> {
    type Row = (String, String, String, bool, i64, Option<i64>, Option<i64>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, name, prefix, all_sites, created_at, last_used_at, revoked_at FROM api_keys
         WHERE owner_id = ?1 AND (?2 IS NULL OR id = ?2) ORDER BY created_at DESC, id",
    )
    .bind(owner.as_str())
    .bind(only.map(ApiKeyId::as_str))
    .fetch_all(&mut *conn)
    .await?;
    let grants: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT o.key_id, o.operation, 1 FROM api_key_operations o
           JOIN api_keys k ON k.id = o.key_id WHERE k.owner_id = ?1 AND (?2 IS NULL OR k.id = ?2)
         UNION ALL
         SELECT s.key_id, s.site_id, 0 FROM api_key_sites s
           JOIN api_keys k ON k.id = s.key_id WHERE k.owner_id = ?1 AND (?2 IS NULL OR k.id = ?2)
         ORDER BY 2",
    )
    .bind(owner.as_str())
    .bind(only.map(ApiKeyId::as_str))
    .fetch_all(conn)
    .await?;
    let mut operations: HashMap<String, Vec<String>> = HashMap::new();
    let mut sites: HashMap<String, Vec<SiteId>> = HashMap::new();
    for (key, value, is_operation) in grants {
        if is_operation {
            operations.entry(key).or_default().push(value);
        } else {
            sites.entry(key).or_default().push(SiteId::new(value));
        }
    }
    Ok(rows
        .into_iter()
        .map(
            |(id, name, prefix, all_sites, created_at, last_used_at, revoked_at)| ApiKey {
                operations: operations.remove(&id).unwrap_or_default(),
                sites: if all_sites {
                    KeySites::All
                } else {
                    KeySites::Only {
                        sites: sites.remove(&id).unwrap_or_default(),
                    }
                },
                id: ApiKeyId::new(id),
                name,
                prefix,
                created_at,
                last_used_at,
                revoked_at,
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::key_need;
    use holocron_protocol::operations;

    #[test]
    fn every_key_grantable_operation_names_its_capability() {
        let missing: Vec<&str> = operations::all()
            .filter(|op| op.policy.key_grantable && key_need(op.name).is_none())
            .map(|op| op.name)
            .collect();
        assert!(missing.is_empty(), "no capability for {missing:?}");
    }
}
