//! Session and identity policy of a site. Control storage owns both (decision 0008). Analytics
//! keeps a copy of the session policy versions, and [`mirror`] is the only code that writes it.

use chrono_tz::Tz;
use holocron_analytics::identity::{PolicyVersion, SessionPolicy, SitePolicies};
use holocron_analytics::jobs;
use holocron_storage::{Id, Storage};
use sqlx::{Executor, Sqlite, SqliteConnection};
use uuid::Uuid;

use crate::db::Db;

/// Every session policy version of `site`, or `None` for an unknown site.
pub async fn session_policies<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    site: &str,
) -> Result<Option<SitePolicies>, sqlx::Error> {
    let rows: Vec<(i32, i32, i64, String, i64)> = sqlx::query_as(
        "SELECT version, epoch, effective_at, timezone, window_minutes
         FROM session_policies WHERE site_id = ? ORDER BY version",
    )
    .bind(site)
    .fetch_all(conn)
    .await?;
    let versions = rows
        .into_iter()
        .map(|(version, epoch, effective_ms, timezone, window)| {
            let invalid = |e: String| sqlx::Error::Decode(e.into());
            let timezone: Tz = timezone
                .parse()
                .map_err(|e: chrono_tz::ParseError| invalid(e.to_string()))?;
            let window = u16::try_from(window).map_err(|e| invalid(e.to_string()))?;
            let policy =
                SessionPolicy::new(timezone, window).map_err(|e| invalid(e.to_string()))?;
            Ok(PolicyVersion {
                version,
                epoch,
                effective_ms,
                policy,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(SitePolicies::from_versions(versions))
}

pub async fn insert_session_policy(
    conn: &mut SqliteConnection,
    site: &str,
    version: &PolicyVersion,
    now_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(site)
    .bind(version.version)
    .bind(version.epoch)
    .bind(version.effective_ms)
    .bind(version.policy.timezone().name())
    .bind(i64::from(version.policy.window_minutes()))
    .bind(now_ms)
    .execute(conn)
    .await?;
    Ok(())
}

/// Copies the session policy versions `conn` sees for `site` into analytics, marking the
/// identity days they change dirty. Runs inside a policy command before its commit, so a
/// report read right after the command sees the new version.
pub async fn mirror(
    conn: &mut SqliteConnection,
    storage: &Storage,
    site: &str,
) -> anyhow::Result<()> {
    let policies = session_policies(&mut *conn, site)
        .await?
        .ok_or_else(|| anyhow::anyhow!("site {site} has no session policy"))?;
    jobs::record_policies(storage, analytics_id(site)?, policies).await?;
    Ok(())
}

/// Repairs the analytical copy of every site at startup. A crash or failed commit between
/// the two stores leaves at most one site's copy behind; this brings it back to the owner.
pub async fn mirror_all(db: &Db, storage: &Storage) -> anyhow::Result<()> {
    let sites: Vec<String> = sqlx::query_scalar("SELECT id FROM sites")
        .fetch_all(&db.reader)
        .await?;
    let mut conn = db.reader.acquire().await?;
    for site in sites {
        mirror(&mut conn, storage, &site).await?;
    }
    Ok(())
}

/// Persistent identity on or off, and the identity policy version.
pub async fn identity_policy(
    conn: &mut SqliteConnection,
    site: &str,
) -> Result<(bool, i32), sqlx::Error> {
    sqlx::query_as("SELECT persistent_identity, identity_policy_version FROM sites WHERE id = ?")
        .bind(site)
        .fetch_one(conn)
        .await
}

/// The 16-byte form analytics records use for a site ID.
pub fn analytics_id(site: &str) -> anyhow::Result<Id> {
    Ok(Uuid::parse_str(site)?.as_u128())
}
