//! Visitor pseudonyms at collection time (decision 0001, Keyed derivation and Key retention).
//!
//! Raw IP, user agent and distinct id enter [`Pseudonyms::derive`] and leave as a 16-byte daily
//! pseudonym. Nothing raw is stored or logged. Day keys are random per source, timezone epoch
//! and local day, created on first use and destroyed at `end(D) + 25h`. An event whose day key
//! is gone, or past its destruction time, is keyless: it carries no daily pseudonym.

use std::collections::HashMap;
use std::sync::Mutex;

use holocron_analytics::identity::{IdentityDay, SitePolicies};
use holocron_storage::Id;
use uuid::Uuid;

use crate::db::Db;

/// Transient identity inputs of one request.
#[derive(Debug, Clone, Copy)]
pub enum Visitor<'a> {
    /// Canonical client IP text and the `User-Agent` header, each possibly empty.
    Anonymous { ip: &'a str, user_agent: &'a str },
    /// The trimmed, truncated `id` of an identify call or of a later event carrying it.
    Identified { distinct_id: &'a str },
}

/// What an event stores about its visitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// Identity day in the source's timezone, days since 1970-01-01.
    pub local_day: i32,
    /// `None` for keyless events.
    pub daily_id: Option<Id>,
}

#[derive(Clone, Copy)]
enum DayKey {
    Live { key: [u8; 32], destroy_after: i64 },
    Destroyed,
}

pub struct Pseudonyms {
    db: Db,
    root: [u8; 32],
    keys: Mutex<HashMap<(Id, i32, i32), DayKey>>,
}

impl Pseudonyms {
    pub fn new(db: Db, root: [u8; 32]) -> Self {
        Self {
            db,
            root,
            keys: Mutex::new(HashMap::new()),
        }
    }

    /// The identity day of `event_time_ms` under the source's session policies and its daily
    /// pseudonym, creating the day key on first use. `event_time_ms` must lie within the
    /// storable event-time range.
    pub async fn derive(
        &self,
        source: Id,
        days: &SitePolicies,
        event_time_ms: i64,
        now_ms: i64,
        visitor: Visitor<'_>,
    ) -> Result<Identity, sqlx::Error> {
        let day = days.identity_day(event_time_ms);
        let daily_id = self
            .day_key(source, day, days.key_destroyed_at(day), now_ms)
            .await?
            .map(|key| daily_pseudonym(&key, visitor));
        Ok(Identity {
            local_day: day.local_day(),
            daily_id,
        })
    }

    /// Destroys every day key whose day ended more than 25 hours ago. Rows stay as tombstones.
    pub async fn destroy_expired(&self, now_ms: i64) -> Result<u64, sqlx::Error> {
        let destroyed = sqlx::query(
            "UPDATE identity_day_keys SET key = NULL, destroyed_at = ?
             WHERE key IS NOT NULL AND destroy_after <= ?",
        )
        .bind(now_ms)
        .bind(now_ms)
        .execute(&self.db.writer)
        .await?
        .rows_affected();
        self.keys
            .lock()
            .expect("day keys")
            .retain(|_, key| match key {
                DayKey::Live { destroy_after, .. } => *destroy_after > now_ms,
                DayKey::Destroyed => true,
            });
        Ok(destroyed)
    }

    /// Persistent profile ID for a consented `distinct_id` (0001). Never equal to a daily
    /// pseudonym by construction: it uses a separate key namespace.
    pub fn profile_id(&self, site: Id, distinct_id: &str) -> Id {
        let context = format!(
            "holocron/v1/site-persistent/{}",
            Uuid::from_u128(site).hyphenated()
        );
        let key = blake3::derive_key(&context, &self.root);
        truncate(blake3::keyed_hash(&key, distinct_id.as_bytes()))
    }

    async fn day_key(
        &self,
        source: Id,
        day: IdentityDay,
        destroy_after: i64,
        now_ms: i64,
    ) -> Result<Option<[u8; 32]>, sqlx::Error> {
        if now_ms >= destroy_after {
            // Past destruction time: keyless, whether or not the job has run yet.
            return Ok(None);
        }
        let slot = (source, day.epoch, day.local_day());
        if let Some(cached) = self.keys.lock().expect("day keys").get(&slot).copied() {
            return Ok(match cached {
                DayKey::Live { key, .. } => Some(key),
                DayKey::Destroyed => None,
            });
        }

        let mut fresh = [0u8; 32];
        getrandom::fill(&mut fresh).expect("operating system randomness");
        let source = Uuid::from_u128(source).hyphenated().to_string();
        // Concurrent first uses race on the insert; every one reads back the winning key.
        sqlx::query(
            "INSERT OR IGNORE INTO identity_day_keys
                 (site_id, timezone_epoch, local_day, key, created_at, destroy_after)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&source)
        .bind(day.epoch)
        .bind(day.local_day())
        .bind(&fresh[..])
        .bind(now_ms)
        .bind(destroy_after)
        .execute(&self.db.writer)
        .await?;
        let stored: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT key FROM identity_day_keys WHERE site_id = ? AND timezone_epoch = ? AND local_day = ?",
        )
        .bind(&source)
        .bind(day.epoch)
        .bind(day.local_day())
        .fetch_one(&self.db.writer)
        .await?;

        let entry = match stored {
            Some(bytes) => DayKey::Live {
                key: bytes.try_into().expect("day keys are 32 bytes"),
                destroy_after,
            },
            None => DayKey::Destroyed,
        };
        self.keys.lock().expect("day keys").insert(slot, entry);
        Ok(match entry {
            DayKey::Live { key, .. } => Some(key),
            DayKey::Destroyed => None,
        })
    }
}

fn daily_pseudonym(day_key: &[u8; 32], visitor: Visitor<'_>) -> Id {
    let mut hasher = blake3::Hasher::new_keyed(day_key);
    match visitor {
        Visitor::Anonymous { ip, user_agent } => {
            hasher.update(&[0]);
            hasher.update(ip.as_bytes());
            hasher.update(&[0]);
            hasher.update(user_agent.as_bytes());
        }
        Visitor::Identified { distinct_id } => {
            hasher.update(&[1]);
            hasher.update(distinct_id.as_bytes());
        }
    }
    truncate(hasher.finalize())
}

fn truncate(hash: blake3::Hash) -> Id {
    let bytes: [u8; 16] = hash.as_bytes()[..16].try_into().expect("16 of 32 bytes");
    Id::from_be_bytes(bytes)
}

/// Reads the installation's root secret, creating it on first start (0001).
pub async fn root_secret(db: &Db, now_ms: i64) -> Result<[u8; 32], sqlx::Error> {
    let mut fresh = [0u8; 32];
    getrandom::fill(&mut fresh).expect("operating system randomness");
    sqlx::query(
        "INSERT OR IGNORE INTO installation_secret (id, root, created_at) VALUES (1, ?, ?)",
    )
    .bind(&fresh[..])
    .bind(now_ms)
    .execute(&db.writer)
    .await?;
    let root: Vec<u8> = sqlx::query_scalar("SELECT root FROM installation_secret WHERE id = 1")
        .fetch_one(&db.writer)
        .await?;
    Ok(root.try_into().expect("root secret is 32 bytes"))
}
