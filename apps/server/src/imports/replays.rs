//! Umami 3.4.0 replays into `recordings.sqlite`, keyed as 0011 says: site, Umami visit ID as
//! the capture, chunk index as the part. The store answers a part it already has as a duplicate,
//! so a chunk repeated after a crash adds nothing. Unsaved replays older than the 14-day replay
//! retention are skipped, as maintenance would remove them; saved replays are kept and saved.

use std::collections::BTreeSet;
use std::io::Read;

use serde_json::json;
use serde_json::value::RawValue;
use sqlx::Row;

use super::Ctx;
use super::events::uuid;
use super::signature::Layout;
use super::source::{Arg, Kind, RowExt};
use super::state::{self, Entry, Progress};
use crate::init::Failure;
use crate::recordings::store::{Client, Part, Saved, Stored};
use crate::recordings::{self, replay};

pub const STEP: &str = "replays";
/// Replay rows hold up to a few megabytes each, so their chunks are small.
const CHUNK_ROWS: i64 = 50;
/// The level collection compresses parts with (0011).
const ZSTD_LEVEL: i32 = 3;

pub async fn import(ctx: &mut Ctx) -> Result<(), Failure> {
    if ctx.layout != Layout::Umami3Postgresql {
        return Ok(());
    }
    let mut step = state::step(&ctx.db, STEP).await.map_err(super::internal)?;
    if step.done {
        return Ok(());
    }
    let saved_rows = ctx
        .source
        .table(
            "session_replay_saved",
            &[
                ("saved_replay_id", Kind::Text),
                ("name", Kind::Text),
                ("website_id", Kind::Text),
                ("visit_id", Kind::Text),
                ("created_at", Kind::Ms),
            ],
            "",
            "saved_replay_id",
            &[],
        )
        .await?;
    let saved: BTreeSet<(u128, u128)> = saved_rows
        .iter()
        .map(|r| {
            (
                uuid(r.text("website_id").unwrap_or_default()),
                uuid(r.text("visit_id").unwrap_or_default()),
            )
        })
        .collect();
    let sites: BTreeSet<u128> = sqlx::query_scalar::<_, String>("SELECT id FROM sites")
        .fetch_all(&ctx.db.reader)
        .await
        .map_err(super::internal)?
        .iter()
        .map(|id| uuid(id))
        .collect();
    let cutoff = recordings::replay_cutoff_ms(ctx.timezone, ctx.clock_ms);
    let policies = holocron_analytics::identity::SitePolicies::new(
        holocron_analytics::identity::SessionPolicy::new(ctx.timezone, 30)
            .map_err(super::internal)?,
    );
    let e = ctx.source.engine;
    let select = format!(
        "SELECT {}, {}, {}, {}, {}, r.events, {}, {}, {}, {}, {}, {}
         FROM session_replay r LEFT JOIN session s ON s.session_id = r.session_id
         WHERE r.replay_id > {} ORDER BY r.replay_id LIMIT ?",
        e.text("r.replay_id"),
        e.text("r.website_id"),
        e.text("r.session_id"),
        e.text("r.visit_id"),
        e.int("r.chunk_index"),
        e.ms("r.started_at"),
        e.ms("r.created_at"),
        e.text("s.browser"),
        e.text("s.os"),
        e.text("s.device"),
        e.text("s.country"),
        e.id_param(),
    );
    let mut cursor = step
        .cursor
        .clone()
        .unwrap_or_else(|| "00000000-0000-0000-0000-000000000000".into());
    loop {
        let rows = ctx
            .source
            .rows(&select, &[Arg::Text(&cursor), Arg::Int(CHUNK_ROWS)])
            .await?;
        let number = step.chunks + 1;
        let Some(last) = rows.last() else { break };
        let last: String = last.try_get(0).map_err(super::source_error)?;
        let mut progress = Progress::default();
        let mut inventory = Vec::new();
        for row in &rows {
            progress.rows += 1;
            let get = |i: usize| {
                row.try_get::<Option<String>, _>(i)
                    .map_err(super::source_error)
            };
            let id = get(0)?.unwrap_or_default();
            let site = uuid(&get(1)?.unwrap_or_default());
            let session = uuid(&get(2)?.unwrap_or_default());
            let capture = uuid(&get(3)?.unwrap_or_default());
            let chunk: i64 = row.try_get(4).map_err(super::source_error)?;
            let bytes: Vec<u8> = row.try_get(5).map_err(super::source_error)?;
            let started: i64 = row.try_get(6).map_err(super::source_error)?;
            let received: i64 = row
                .try_get::<Option<i64>, _>(7)
                .map_err(super::source_error)?
                .unwrap_or(started);
            if !sites.contains(&site) || (started < cutoff && !saved.contains(&(site, capture))) {
                progress.skipped += 1;
                continue;
            }
            let Some((json, summary)) = events(&bytes) else {
                inventory.push(Entry::new(
                    "session_replay",
                    &id,
                    "replay_unreadable",
                    json!({ "replay_id": id, "website_id": get(1)?, "visit_id": get(3)?, "chunk_index": chunk,
                            "events": data_encoding::BASE64.encode(&bytes) }),
                ));
                progress.skipped += 1;
                continue;
            };
            let payload =
                zstd::bulk::compress(json.as_bytes(), ZSTD_LEVEL).map_err(super::internal)?;
            let fingerprint: [u8; 16] = blake3::hash(json.as_bytes()).as_bytes()[..16]
                .try_into()
                .expect("16 bytes");
            let local_day = policies.identity_day(started).local_day();
            let client = Client {
                browser: get(8)?,
                os: get(9)?,
                device: get(10)?,
                country: get(11)?,
            };
            let part = Part {
                site,
                capture,
                visitor: Some(ctx.daily_id(site, session, local_day)),
                chunk,
                received_at: received,
                summary: &summary,
                raw_bytes: json.len() as i64,
                payload,
                fingerprint,
                client: &client,
            };
            match ctx
                .store
                .put(part, &[], recordings::DEFAULT_CAP_BYTES)
                .await
                .map_err(super::internal)?
            {
                Stored::Accepted | Stored::Duplicate => progress.imported += 1,
                Stored::Excluded | Stored::OverCap => {
                    inventory.push(Entry::new(
                        "session_replay",
                        &id,
                        "replay_over_cap",
                        json!({ "replay_id": id, "website_id": get(1)?, "visit_id": get(3)?, "chunk_index": chunk }),
                    ));
                    progress.skipped += 1;
                }
            }
        }
        checkpoint(ctx, &last, number, progress, &inventory, false).await?;
        cursor = last;
        step.chunks = number;
    }

    let mut inventory = Vec::new();
    for row in saved_rows {
        let id = row.text("saved_replay_id").unwrap_or_default().to_owned();
        let site = uuid(row.text("website_id").unwrap_or_default());
        let capture = uuid(row.text("visit_id").unwrap_or_default());
        let at = row.int("created_at").unwrap_or(ctx.clock_ms);
        let cap = recordings::DEFAULT_CAP_BYTES / 5;
        let saved = ctx
            .store
            .save(site, capture, at, cap)
            .await
            .map_err(super::internal)?;
        let reason = match saved {
            Saved::Saved(_) => "partially_mapped:name",
            Saved::NotFound => "replay_not_imported",
            Saved::OverBudget => "saved_replay_budget_exceeded",
        };
        inventory.push(Entry::new("session_replay_saved", id, reason, row.into()));
    }
    checkpoint(
        ctx,
        &cursor,
        step.chunks + 1,
        Progress::default(),
        &inventory,
        true,
    )
    .await
}

async fn checkpoint(
    ctx: &Ctx,
    cursor: &str,
    number: i64,
    progress: Progress,
    inventory: &[Entry],
    done: bool,
) -> Result<(), Failure> {
    let mut tx = ctx
        .db
        .writer
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(super::internal)?;
    state::record(&mut tx, inventory)
        .await
        .map_err(super::internal)?;
    state::checkpoint(&mut tx, STEP, cursor, number, progress, done)
        .await
        .map_err(super::internal)?;
    tx.commit().await.map_err(super::internal)?;
    ctx.progress.checkpoint(STEP, progress.rows, done);
    Ok(())
}

/// The gzip-compressed JSON event array Umami stores, re-joined as collection joins a part,
/// and its summary. `None` when it is not one.
fn events(bytes: &[u8]) -> Option<(String, replay::Summary)> {
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_string(&mut text)
        .ok()?;
    let events: Vec<&RawValue> = serde_json::from_str(&text).ok()?;
    let summary = replay::summarize(&events).ok()?;
    let joined: Vec<&str> = events.iter().map(|e| e.get()).collect();
    Some((format!("[{}]", joined.join(",")), summary))
}
