//! Stored command answers. A key binds owner, operation and argument fingerprint; only
//! committed answers are stored, inside the command's own transaction.

use holocron_protocol::limits::IDEMPOTENCY_WINDOW_MS;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

pub struct Stored {
    pub operation: String,
    pub fingerprint: String,
    pub response: String,
    /// JSON list of the scopes the command resolved, checked again before a replay.
    pub authorized: String,
    /// The command passed a reauthentication check; a replay needs a fresh one too.
    pub fresh_login: bool,
}

/// A stored answer younger than the replay window.
pub async fn find(
    conn: &mut SqliteConnection,
    owner: &str,
    key: &str,
    now: i64,
) -> Result<Option<Stored>, sqlx::Error> {
    let row: Option<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT operation, fingerprint, response, authorized, fresh_login FROM idempotency_keys
         WHERE owner = ? AND key = ? AND created_at > ?",
    )
    .bind(owner)
    .bind(key)
    .bind(now - IDEMPOTENCY_WINDOW_MS)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(
        |(operation, fingerprint, response, authorized, fresh_login)| Stored {
            operation,
            fingerprint,
            response,
            authorized,
            fresh_login,
        },
    ))
}

/// Stores an answer and drops expired ones, in the caller's transaction.
pub async fn store(
    conn: &mut SqliteConnection,
    owner: &str,
    key: &str,
    stored: &Stored,
    now: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM idempotency_keys WHERE created_at <= ?")
        .bind(now - IDEMPOTENCY_WINDOW_MS)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "INSERT OR REPLACE INTO idempotency_keys
         (owner, key, operation, fingerprint, response, authorized, fresh_login, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(owner)
    .bind(key)
    .bind(&stored.operation)
    .bind(&stored.fingerprint)
    .bind(&stored.response)
    .bind(&stored.authorized)
    .bind(stored.fresh_login)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// SHA-256 of the arguments in canonical form: object keys sorted, no whitespace.
pub fn fingerprint(args: &Value) -> String {
    let mut canonical = String::new();
    write_canonical(args, &mut canonical);
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                write_canonical(value, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::fingerprint;

    #[test]
    fn fingerprint_ignores_key_order() {
        assert_eq!(
            fingerprint(&json!({ "a": 1, "b": { "c": [1, 2], "d": "x" } })),
            fingerprint(&json!({ "b": { "d": "x", "c": [1, 2] }, "a": 1 }))
        );
        assert_ne!(
            fingerprint(&json!({ "a": 1 })),
            fingerprint(&json!({ "a": 2 }))
        );
    }
}
