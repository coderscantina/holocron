//! Topic event logs. A topic's head is its revision; snapshots and commands read it inside
//! their own transaction.

use holocron_protocol::envelope::Topic;
use sqlx::SqliteConnection;

pub async fn head(conn: &mut SqliteConnection, topic: &Topic) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM topic_events WHERE topic = ?")
        .bind(topic.as_str())
        .fetch_one(conn)
        .await
}

/// Appends an event and returns its sequence. Callers hold the write transaction.
pub async fn record(
    conn: &mut SqliteConnection,
    topic: &Topic,
    kind: &str,
    payload: &serde_json::Value,
    now: i64,
) -> Result<i64, sqlx::Error> {
    let seq = head(&mut *conn, topic).await? + 1;
    sqlx::query(
        "INSERT INTO topic_events (topic, seq, kind, payload, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(topic.as_str())
    .bind(seq)
    .bind(kind)
    .bind(payload.to_string())
    .bind(now)
    .execute(conn)
    .await?;
    Ok(seq)
}
