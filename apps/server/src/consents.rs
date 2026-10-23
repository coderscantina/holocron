//! Consent intervals of persistent profiles (decision 0001, Persistent consent transport and
//! Revocation). Instants are event times, so a late event links by when it happened.

use holocron_storage::Id;
use sqlx::SqliteConnection;

use crate::db::Db;

/// Opens a consent interval at `at_ms` unless one is open; an open one is kept as it is.
pub async fn grant(db: &Db, site: &str, profile: Id, at_ms: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO consents (site_id, profile_id, granted_at) VALUES (?, ?, ?)",
    )
    .bind(site)
    .bind(&profile.to_be_bytes()[..])
    .bind(at_ms)
    .execute(&db.writer)
    .await?;
    Ok(())
}

/// Closes the open interval of `profile` at `at_ms`, or at its grant when that came later.
pub async fn revoke(db: &Db, site: &str, profile: Id, at_ms: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE consents SET revoked_at = max(?, granted_at)
         WHERE site_id = ? AND profile_id = ? AND revoked_at IS NULL",
    )
    .bind(at_ms)
    .bind(site)
    .bind(&profile.to_be_bytes()[..])
    .execute(&db.writer)
    .await?;
    Ok(())
}

/// Closes every open interval of the site: persistent identity was turned off.
pub async fn revoke_all(
    conn: &mut SqliteConnection,
    site: &str,
    at_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE consents SET revoked_at = max(?, granted_at) WHERE site_id = ? AND revoked_at IS NULL")
        .bind(at_ms)
        .bind(site)
        .execute(conn)
        .await?;
    Ok(())
}

/// Whether consent was active at `event_time_ms`: `granted_at <= event_time < revoked_at`.
pub async fn active(
    db: &Db,
    site: &str,
    profile: Id,
    event_time_ms: i64,
) -> Result<bool, sqlx::Error> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM consents
         WHERE site_id = ? AND profile_id = ? AND granted_at <= ? AND (revoked_at IS NULL OR ? < revoked_at)
         LIMIT 1",
    )
    .bind(site)
    .bind(&profile.to_be_bytes()[..])
    .bind(event_time_ms)
    .bind(event_time_ms)
    .fetch_optional(&db.reader)
    .await?;
    Ok(found.is_some())
}
