//! `holocron diagnostics` (CAP-OPS-10): one JSON document an operator can attach to a bug
//! report. It is built field by field from counts, sizes, versions, states and fixed names.
//! Nothing from an event, a request, an identity input, a credential or an error message is
//! read into it, so there is nothing to scrub afterwards.

use holocron_protocol::operations::collection::{CounterCount, OutcomeCount};
use holocron_storage::{Disk, INBOX_FORMAT, Lane};
use serde::Serialize;
use serde_json::Value;

use super::health::{self, Health};
use super::internal;
use super::versions::{self, Versions};
use crate::init::Failure;
use crate::state::AppState;

/// Maintenance jobs listed, newest first.
const JOBS_LISTED: i64 = 20;

#[derive(Serialize)]
struct Bundle {
    holocron: &'static str,
    generated_at_ms: i64,
    clock: &'static str,
    schema: Schema,
    health: Health,
    disk: Disk,
    inbox: Inbox,
    analytics: Analytics,
    recording: Recording,
    collection: Collection,
    jobs: Vec<Job>,
    settings: Settings,
}

#[derive(Serialize)]
struct Schema {
    versions: Versions,
    inbox_format: i64,
}

#[derive(Serialize)]
struct Inbox {
    records: u64,
    bytes: u64,
    oldest_age_ms: u64,
}

#[derive(Serialize)]
struct Analytics {
    watermark_batch: i64,
    watermark_sequence: i64,
    transfer_failures: u64,
    transfer_failing_since_ms: Option<i64>,
    /// Identity days waiting for correction, and the oldest batch that marked one.
    dirty_days: i64,
    oldest_dirty_batch: Option<i64>,
}

#[derive(Serialize)]
struct Recording {
    bytes: i64,
    saved_bytes: i64,
    cap_bytes: u64,
    captures: i64,
    file_bytes: i64,
    refusing: bool,
}

#[derive(Serialize)]
struct Collection {
    since_ms: i64,
    outcomes: Vec<OutcomeCount>,
    counters: Vec<CounterCount>,
}

#[derive(Serialize, sqlx::FromRow)]
struct Job {
    kind: String,
    state: String,
    created_at: i64,
    finished_at: Option<i64>,
    /// The outcome name of a failed job; its message is left out.
    failure: Option<String>,
}

/// Settings that explain behavior, as values or counts. Addresses and URLs are left out.
#[derive(Serialize)]
struct Settings {
    disk_reserve_bytes: u64,
    recording_cap_bytes: u64,
    public_url_set: bool,
    trusted_proxies: usize,
    ignored_networks: usize,
    web_app_served: bool,
}

pub async fn collect(state: &AppState) -> Result<Value, Failure> {
    let collector = &state.collector;
    let storage = collector.storage();
    let now = collector.clock().now_ms();
    let status = storage.status();
    let (dirty_days, oldest_dirty_batch) = storage
        .read(Lane::Live, |s| {
            s.connection()
                .query_row("SELECT count(*), min(batch_id) FROM dirty_days", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
        })
        .await
        .map_err(internal)?
        .value;
    let usage = state.recordings.store.usage().await.map_err(internal)?;
    let jobs: Vec<Job> = sqlx::query_as(
        "SELECT kind, state, created_at, finished_at,
                CASE WHEN state = 'failed' THEN json_extract(result, '$.code') END AS failure
         FROM maintenance_jobs ORDER BY created_at DESC LIMIT ?",
    )
    .bind(JOBS_LISTED)
    .fetch_all(&state.db.reader)
    .await
    .map_err(internal)?;
    let config = &state.config;
    let bundle = Bundle {
        holocron: env!("CARGO_PKG_VERSION"),
        generated_at_ms: now,
        clock: if collector.clock().is_test() {
            "test"
        } else {
            "system"
        },
        schema: Schema {
            versions: versions::release(),
            inbox_format: INBOX_FORMAT,
        },
        health: health::check(state).await,
        disk: storage.disk().map_err(internal)?,
        inbox: Inbox {
            records: status.inbox_records,
            bytes: status.inbox_bytes,
            oldest_age_ms: storage.inbox_age().await.map_err(internal)?.as_millis() as u64,
        },
        analytics: Analytics {
            watermark_batch: status.watermark.batch,
            watermark_sequence: status.watermark.sequence,
            transfer_failures: status.transfer_failures,
            transfer_failing_since_ms: status.transfer_failing_since_ms,
            dirty_days,
            oldest_dirty_batch,
        },
        recording: Recording {
            bytes: usage.bytes,
            saved_bytes: usage.saved_bytes,
            cap_bytes: state.recordings.cap_bytes(),
            captures: usage.captures,
            file_bytes: usage.file_bytes,
            refusing: state.recordings.refusing(now),
        },
        collection: Collection {
            since_ms: collector.metrics().since(),
            outcomes: collector.metrics().outcomes(),
            counters: collector.metrics().counters(),
        },
        jobs,
        settings: Settings {
            disk_reserve_bytes: config.disk_reserve_bytes,
            recording_cap_bytes: config.collect.recording_cap_bytes,
            public_url_set: config.public_origin.is_some(),
            trusted_proxies: config.collect.trusted_proxies.len(),
            ignored_networks: config.collect.ignore_ips.len(),
            web_app_served: config.web_dir.is_some(),
        },
    };
    Ok(serde_json::to_value(bundle).expect("diagnostics serialize"))
}
