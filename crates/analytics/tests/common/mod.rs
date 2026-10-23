//! Builds canonical records the way collection does (identity day from the site
//! policy, daily pseudonym per identity day, profile when linked) and reads
//! derived sessions back by member event IDs.
#![allow(dead_code)]

use std::time::Duration;

use chrono_tz::Tz;
use holocron_analytics::Limits;
use holocron_analytics::identity::{SessionPolicy, SitePolicies};
use holocron_storage::{Budgets, Dimensions, Event, EventKind, Lane, Record, Revocation, Storage};

pub fn ms(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap_or_else(|e| panic!("{rfc3339}: {e}"))
        .timestamp_millis()
}

pub fn budgets() -> Budgets {
    Budgets {
        transfer_delay: Duration::from_millis(2),
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

pub async fn open() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path(), budgets()).await.unwrap();
    (dir, storage)
}

/// Stand-in for a pseudonym: a stable 128-bit FNV-1a hash of its label.
pub fn pseudonym(label: &str) -> u128 {
    label
        .bytes()
        .fold(0x6c62272e07bb014262b821756295c58d, |h, b| {
            (h ^ u128::from(b)).wrapping_mul(0x0000000001000000000000000000013B)
        })
}

/// Who sent an event, as collection resolves it (0001 Daily identity of every event).
#[derive(Debug, Clone, Copy)]
pub enum Who<'a> {
    /// Anonymous inputs (IP and user agent), labelled.
    Anon(&'a str),
    /// `identify(id)` without active consent: the daily identified pseudonym.
    DailyId(&'a str),
    /// Consent active at event time: daily identified pseudonym plus profile.
    Profile(&'a str),
    /// The day key was destroyed before arrival.
    Keyless,
}

pub struct Site {
    pub id: u128,
    pub policies: SitePolicies,
    next_id: u128,
}

impl Site {
    pub fn new(id: u128, timezone: Tz) -> Self {
        Self {
            id,
            policies: SitePolicies::new(SessionPolicy::new(timezone, 30).unwrap()),
            next_id: id << 32,
        }
    }

    pub fn event(&mut self, kind: EventKind, at: &str, who: Who) -> Record {
        let time = ms(at);
        self.event_at(kind, time, time, who)
    }

    /// An event with an explicit receive time, for late arrivals.
    pub fn event_at(&mut self, kind: EventKind, time: i64, received: i64, who: Who) -> Record {
        self.next_id += 1;
        let day = self.policies.identity_day(time);
        let daily = |label: String| {
            Some(pseudonym(&format!(
                "{label} {} epoch {}",
                day.date, day.epoch
            )))
        };
        let (daily_id, profile_id) = match who {
            Who::Anon(label) => (daily(format!("anon {label}")), None),
            Who::DailyId(id) => (daily(format!("daily-id {id}")), None),
            Who::Profile(id) => (
                daily(format!("daily-id {id}")),
                Some(pseudonym(&format!("profile {id}"))),
            ),
            Who::Keyless => (None, None),
        };
        Record::Event(Event {
            id: self.next_id,
            site: self.id,
            kind,
            event_time_ms: time,
            receive_time_ms: received,
            local_day: day.local_day(),
            daily_id,
            profile_id,
            client_event_id: None,
            dimensions: Dimensions {
                path: Some("/".into()),
                event_name: (kind == EventKind::CustomEvent).then(|| "signup".into()),
                ..Dimensions::default()
            },
            performance: None,
            revenue: None,
            properties: Vec::new(),
        })
    }

    pub fn revocation(&mut self, profile: &str, at: &str) -> Record {
        self.next_id += 1;
        Record::Revocation(Revocation {
            id: self.next_id,
            site: self.id,
            profile_id: Some(pseudonym(&format!("profile {profile}"))),
            revoked_at_ms: ms(at),
            receive_time_ms: ms(at),
        })
    }
}

pub fn id_of(record: &Record) -> u128 {
    match record {
        Record::Event(e) => e.id,
        Record::Identify(i) => i.id,
        Record::Revocation(r) => r.id,
        Record::ReplayFragment(f) => f.id,
        Record::HeatmapInteraction(h) => h.id,
    }
}

pub async fn ingest(storage: &Storage, records: &[Record]) {
    storage.admit(records).await.unwrap();
    storage.drain().await;
}

pub async fn correct(storage: &Storage) {
    holocron_analytics::run_until_clean(storage, Limits::default())
        .await
        .unwrap();
}

/// One derived session: member event IDs in `(event_time, seq)` order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Session {
    pub members: Vec<u128>,
    pub population: u8,
    pub policy: i32,
    pub span_ms: i64,
}

pub async fn sessions(storage: &Storage, site: u128) -> Vec<Session> {
    let mut sessions = storage
        .read(Lane::Report, move |s| {
            let mut statement = s.connection().prepare(
                "SELECT list(e.id ORDER BY e.event_time, e.seq), any_value(s.population), any_value(s.policy_version),
                        any_value(epoch_ms(s.last_time) - epoch_ms(s.start_time))
                 FROM sessions s
                 JOIN session_members m ON m.site = s.site AND m.session_id = s.session_id AND m.role = 1
                 JOIN events e ON e.seq = m.seq
                 WHERE s.site = ?
                 GROUP BY s.session_id",
            )?;
            statement
                .query_map([site], |r| {
                    let members: holocron_storage::duckdb::types::Value = r.get(0)?;
                    Ok(Session {
                        members: ids(members),
                        population: r.get(1)?,
                        policy: r.get(2)?,
                        span_ms: r.get(3)?,
                    })
                })?
                .collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value;
    sessions.sort();
    sessions
}

/// Member IDs of each session, the shape most tests compare.
pub async fn groups(storage: &Storage, site: u128) -> Vec<Vec<u128>> {
    sessions(storage, site)
        .await
        .into_iter()
        .map(|s| s.members)
        .collect()
}

pub fn ids(value: holocron_storage::duckdb::types::Value) -> Vec<u128> {
    use holocron_storage::duckdb::types::Value;
    match value {
        Value::List(items) => items
            .into_iter()
            .map(|v| match v {
                Value::UHugeInt(id) => id,
                // duckdb-rs returns UHUGEINT list elements as HUGEINT.
                Value::HugeInt(id) => id as u128,
                other => panic!("member id {other:?}"),
            })
            .collect(),
        other => panic!("member list {other:?}"),
    }
}
