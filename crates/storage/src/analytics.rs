//! DuckDB analytical store: canonical tables, applied-batch ledger, watermark.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use duckdb::types::{TimeUnit, Value};
use duckdb::{Config, Connection, OptionalExt, ToSql, params};

use crate::failpoint;
use crate::fence::{Erasure, Fence, Fences};
use crate::inbox::Batch;
use crate::record::{INBOX_FORMAT, Id, Interaction, Property, PropertyValue, Record};
use crate::{Budgets, Error, Mode, Watermark};

/// Analytical schema version, recorded in every ledger entry and in `schema_info` (0006 Schema
/// versions). Version 2 added the derived session, policy and aggregate tables; version 3
/// stores event properties in key partitions; version 4 adds `schema_info` with the pairing ID;
/// version 5 adds the visitor coordinates to `events`.
pub const ANALYTICS_SCHEMA: i64 = 5;

/// What a file without `schema_info` is: it was written before milestone 12.
const UNVERSIONED_SCHEMA: i64 = 3;

/// Population code of persistent-profile sessions in `sessions.population`.
const PROFILE_POPULATION: u8 = 2;

/// Event property values live in `event_properties_0` to `event_properties_63`. A key gets
/// one partition when it is first stored, round robin, so a condition on a key scans that
/// key's partition instead of every property of the range (0008 Property partitions). The
/// view `event_properties` unions them.
pub const PROPERTY_PARTS: u16 = 64;

const PROPERTY_COLUMNS: &str = "seq BIGINT NOT NULL, site UHUGEINT NOT NULL, event_time TIMESTAMP NOT NULL,
    key VARCHAR NOT NULL, type UTINYINT NOT NULL,
    v_string VARCHAR, v_integer BIGINT, v_decimal BIGINT, v_bool BOOLEAN, v_date TIMESTAMP, v_array VARCHAR";

const SCHEMA: &str = r#"
-- One row: the schema version, the release that wrote it, and the pairing ID shared with the
-- inbox (decision 0014).
CREATE TABLE IF NOT EXISTS schema_info (
    analytics_schema INTEGER NOT NULL,
    binary_version VARCHAR NOT NULL,
    pair VARCHAR
);
CREATE TABLE IF NOT EXISTS applied_batches (
    batch_id BIGINT PRIMARY KEY,
    first_seq BIGINT NOT NULL,
    last_seq BIGINT NOT NULL,
    records BIGINT NOT NULL,
    analytics_schema INTEGER NOT NULL,
    inbox_format INTEGER NOT NULL,
    applied_at TIMESTAMP NOT NULL DEFAULT current_timestamp
);
CREATE TABLE IF NOT EXISTS events (
    seq BIGINT NOT NULL,
    id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    kind UTINYINT NOT NULL,
    event_time TIMESTAMP NOT NULL,
    receive_time TIMESTAMP NOT NULL,
    local_day DATE NOT NULL,
    daily_id UHUGEINT,
    profile_id UHUGEINT,
    hostname VARCHAR, path VARCHAR, query VARCHAR, title VARCHAR,
    referrer_domain VARCHAR, referrer_path VARCHAR,
    utm_source VARCHAR, utm_medium VARCHAR, utm_campaign VARCHAR, utm_content VARCHAR, utm_term VARCHAR,
    click_id VARCHAR,
    country VARCHAR, region VARCHAR, city VARCHAR, browser VARCHAR, os VARCHAR, device VARCHAR,
    screen VARCHAR, language VARCHAR, event_name VARCHAR, tag VARCHAR,
    lcp INTEGER, fcp INTEGER, ttfb INTEGER, inp INTEGER, cls INTEGER,
    currency VARCHAR, revenue_minor BIGINT,
    latitude SMALLINT, longitude SMALLINT
);
CREATE TABLE IF NOT EXISTS session_properties (
    seq BIGINT NOT NULL,
    id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    event_time TIMESTAMP NOT NULL,
    local_day DATE NOT NULL,
    daily_id UHUGEINT,
    profile_id UHUGEINT,
    key VARCHAR NOT NULL,
    type UTINYINT NOT NULL,
    v_string VARCHAR, v_integer BIGINT, v_decimal BIGINT, v_bool BOOLEAN, v_date TIMESTAMP, v_array VARCHAR
);
CREATE TABLE IF NOT EXISTS replay_fragments (
    seq BIGINT NOT NULL,
    id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    capture_id UHUGEINT NOT NULL,
    chunk_index USMALLINT NOT NULL,
    chunk_total USMALLINT NOT NULL,
    is_final BOOLEAN NOT NULL,
    event_time TIMESTAMP NOT NULL,
    receive_time TIMESTAMP NOT NULL,
    payload BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS heatmap_interactions (
    seq BIGINT NOT NULL,
    id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    capture_id UHUGEINT NOT NULL,
    event_time TIMESTAMP NOT NULL,
    receive_time TIMESTAMP NOT NULL,
    path VARCHAR NOT NULL,
    viewport_width USMALLINT NOT NULL,
    viewport_height USMALLINT NOT NULL,
    kind UTINYINT NOT NULL,
    x USMALLINT, page_y UINTEGER, scroll_percent UTINYINT
);
-- Consent revocations from collection: close open profile sessions (0001 Revocation).
-- `profile_id` NULL revokes every profile of the site (persistent identity disabled).
CREATE TABLE IF NOT EXISTS consent_revocations (
    seq BIGINT NOT NULL,
    id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    profile_id UHUGEINT,
    revoked_at_ms BIGINT NOT NULL
);
-- Identity days whose derived data is stale: written by applied batches and by
-- settings-change and recalculation jobs, cleared by correction (milestone 05).
CREATE TABLE IF NOT EXISTS dirty_days (
    site UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    batch_id BIGINT NOT NULL
);
-- Restricted provenance of imported events (milestone 11): the Umami session, visit and
-- distinct IDs behind each imported event, whose `id` is the source event ID. Never read by
-- reports. `chunk` is the import chunk that wrote the row, so a retried chunk replaces its rows.
CREATE TABLE IF NOT EXISTS import_provenance (
    chunk BIGINT NOT NULL,
    event_id UHUGEINT NOT NULL,
    site UHUGEINT NOT NULL,
    source_session UHUGEINT NOT NULL,
    source_visit UHUGEINT,
    distinct_id VARCHAR
);
-- Imported history per site: results that reach before `last_time` carry the historical
-- limitations of decision 0013. `source` names the snapshot, like `umami-3.4.0-postgresql`.
CREATE TABLE IF NOT EXISTS imported_history (
    site UHUGEINT NOT NULL,
    source VARCHAR NOT NULL,
    first_time TIMESTAMP NOT NULL,
    last_time TIMESTAMP NOT NULL
);
-- Derived data below is rebuilt from the immutable tables above and is never a
-- source of truth. `seq` everywhere is the inbox ingestion sequence.
CREATE TABLE IF NOT EXISTS session_policies (
    site UHUGEINT NOT NULL,
    version INTEGER NOT NULL,
    epoch INTEGER NOT NULL,
    effective_ms BIGINT NOT NULL,
    timezone VARCHAR NOT NULL,
    window_ms BIGINT NOT NULL,
    PRIMARY KEY (site, version)
);
-- `sessions` and `session_members` are written in (site, time) order, and correction leaves
-- rows that did not change in place, so a range read prunes row groups (0008 Filtered reports).
CREATE TABLE IF NOT EXISTS sessions (
    site UHUGEINT NOT NULL,
    -- ingestion sequence of the first member event
    session_id BIGINT NOT NULL,
    population UTINYINT NOT NULL,
    identity UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    start_time TIMESTAMP NOT NULL,
    last_time TIMESTAMP NOT NULL,
    -- last_time plus the window: performance and identify rows before it attach
    ends_at TIMESTAMP NOT NULL,
    pageviews INTEGER NOT NULL,
    custom_events INTEGER NOT NULL,
    policy_version INTEGER NOT NULL,
    -- Session-scoped fields (0004 Field scope), read by filtered reports: the first member
    -- event's values, then the touch's (first pageview, else first event).
    country VARCHAR, region VARCHAR, city VARCHAR, browser VARCHAR, os VARCHAR, device VARCHAR,
    language VARCHAR, screen VARCHAR,
    referrer_domain VARCHAR, referrer_path VARCHAR,
    utm_source VARCHAR, utm_medium VARCHAR, utm_campaign VARCHAR, utm_content VARCHAR, utm_term VARCHAR,
    click_id VARCHAR, touch_query VARCHAR
);
CREATE TABLE IF NOT EXISTS session_members (
    site UHUGEINT NOT NULL,
    seq BIGINT NOT NULL,
    session_id BIGINT NOT NULL,
    role UTINYINT NOT NULL,
    -- the member's event time; an identify's own time
    event_time TIMESTAMP NOT NULL
);
CREATE TABLE IF NOT EXISTS day_aggregates (
    site UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    metric_version INTEGER NOT NULL,
    session_policy_versions INTEGER[] NOT NULL,
    pageviews BIGINT NOT NULL,
    custom_events BIGINT NOT NULL,
    eligible_events BIGINT NOT NULL,
    identified_events BIGINT NOT NULL,
    linked_events BIGINT NOT NULL,
    visitors BIGINT NOT NULL,
    sessions BIGINT NOT NULL,
    pageview_sessions BIGINT NOT NULL,
    bounces BIGINT NOT NULL,
    span_sum_ms BIGINT NOT NULL,
    computed_through BIGINT NOT NULL
);
-- Exact rollups of eligible events per identity day, refreshed with `day_aggregates`: a day
-- with a current `day_aggregates` row has all of them (0008 Rollups).
-- `hour` is the UTC hour of the events; visitors are distinct daily pseudonyms in the cell.
-- First and last event times let a read check that the day's events lie inside its instants.
CREATE TABLE IF NOT EXISTS hour_aggregates (
    site UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    hour TIMESTAMP NOT NULL,
    pageviews BIGINT NOT NULL,
    custom_events BIGINT NOT NULL,
    visitors BIGINT NOT NULL,
    first_time TIMESTAMP NOT NULL,
    last_time TIMESTAMP NOT NULL
);
-- One row per day and value of one dimension; session-scoped fields carry the session's value.
CREATE TABLE IF NOT EXISTS dimension_aggregates (
    site UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    field VARCHAR NOT NULL,
    value VARCHAR,
    pageviews BIGINT NOT NULL,
    custom_events BIGINT NOT NULL,
    visitors BIGINT NOT NULL
);
-- First and last eligible event of each daily pseudonym, for day buckets in another timezone.
CREATE TABLE IF NOT EXISTS visitor_days (
    site UHUGEINT NOT NULL,
    local_day DATE NOT NULL,
    daily_id UHUGEINT NOT NULL,
    first_time TIMESTAMP NOT NULL,
    last_time TIMESTAMP NOT NULL
);
"#;

/// Property value type tags stored in the `type` column.
pub mod property_type {
    pub const NULL: u8 = 0;
    pub const BOOL: u8 = 1;
    pub const INTEGER: u8 = 2;
    pub const DECIMAL: u8 = 3;
    pub const DATE: u8 = 4;
    pub const STRING: u8 = 5;
    pub const ARRAY: u8 = 6;
}

pub(crate) fn open(
    path: &Path,
    spill: &Path,
    budgets: &Budgets,
    mode: Mode,
) -> Result<Connection, Error> {
    let config = Config::default()
        .threads(budgets.engine_threads as i64)?
        .max_memory(&format!("{}B", budgets.engine_memory_bytes))?
        .with("temp_directory", spill.to_str().expect("utf-8 data path"))?
        .with(
            "max_temp_directory_size",
            format!("{}B", budgets.spill_bytes),
        )?
        .with("preserve_insertion_order", "false")?
        // Correction's ASOF joins were planned as nested loops on low estimates and ran for
        // minutes per step at 10% of the workload fixture (0008 Filtered reports).
        .with("asof_loop_join_threshold", "0")?
        .enable_autoload_extension(false)?;
    let conn = Connection::open_with_flags(path, config)?;
    let stored = stored_schema(&conn)?;
    match stored {
        Some(version) if version == ANALYTICS_SCHEMA => return Ok(conn),
        Some(version) if version > ANALYTICS_SCHEMA => {
            return Err(Error::DataNewer(format!(
                "analytics.duckdb has schema {version}, this release {ANALYTICS_SCHEMA}"
            )));
        }
        Some(version) if mode == Mode::Serve => {
            return Err(Error::UpgradeRequired(format!(
                "analytics.duckdb has schema {version}, this release {ANALYTICS_SCHEMA}"
            )));
        }
        _ => {}
    }
    let outdated = match stored {
        Some(_) => migrate(&conn)?,
        None => false,
    };
    conn.execute_batch(SCHEMA)?;
    conn.execute_batch(&property_schema())?;
    if outdated {
        conn.execute_batch(
            "INSERT INTO dirty_days
             SELECT DISTINCT site, local_day, (SELECT coalesce(max(batch_id), 0) FROM applied_batches) FROM events;",
        )?;
    }
    // Written last, so an interrupted upgrade runs the steps above again; each one checks what
    // the file already has.
    let version = params![ANALYTICS_SCHEMA, env!("CARGO_PKG_VERSION")];
    if conn.execute(
        "UPDATE schema_info SET analytics_schema = ?, binary_version = ?",
        version,
    )? == 0
    {
        conn.execute("INSERT INTO schema_info VALUES (?, ?, NULL)", version)?;
    }
    Ok(conn)
}

/// The schema version of an analytics file, `None` for a new, empty one.
pub(crate) fn stored_schema(conn: &Connection) -> duckdb::Result<Option<i64>> {
    let recorded = if table_exists(conn, "schema_info")? {
        conn.query_row("SELECT analytics_schema FROM schema_info", [], |r| r.get(0))
            .optional()?
    } else {
        None
    };
    if recorded.is_some() {
        return Ok(recorded);
    }
    Ok(table_exists(conn, "applied_batches")?.then_some(UNVERSIONED_SCHEMA))
}

fn table_exists(conn: &Connection, table: &str) -> duckdb::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM duckdb_tables() WHERE table_name = ?)",
        [table],
        |r| r.get(0),
    )
}

/// The pairing ID shared with the inbox, `None` before the pair was created or in a file
/// from before pairing.
pub(crate) fn pair(conn: &Connection) -> duckdb::Result<Option<String>> {
    if !table_exists(conn, "schema_info")? {
        return Ok(None);
    }
    Ok(conn
        .query_row("SELECT pair FROM schema_info", [], |r| r.get(0))
        .optional()?
        .flatten())
}

pub(crate) fn set_pair(conn: &Connection, pair: &str) -> duckdb::Result<()> {
    conn.execute("UPDATE schema_info SET pair = ?", [pair])?;
    Ok(())
}

/// Brings a file of an older schema to the current tables. Returns whether the derived tables
/// were dropped, so every day with events must be rebuilt.
fn migrate(conn: &Connection) -> Result<bool, Error> {
    // Files before schema 5 have no visitor coordinates; the columns go last, as in `SCHEMA`.
    conn.execute_batch(
        "ALTER TABLE events ADD COLUMN IF NOT EXISTS latitude SMALLINT;
         ALTER TABLE events ADD COLUMN IF NOT EXISTS longitude SMALLINT;",
    )?;
    // A file from before the key partitions keeps every event property in one table: its rows
    // move to their partitions, keys assigned in order of first appearance as at apply.
    if table_exists(conn, "event_properties")? {
        let mut sql = format!(
            "BEGIN;
             ALTER TABLE event_properties RENAME TO event_properties_unpartitioned;
             {}
             INSERT INTO property_keys
             SELECT key, (row_number() OVER (ORDER BY min(seq), key) - 1) % {PROPERTY_PARTS}
             FROM event_properties_unpartitioned GROUP BY key;",
            property_schema()
        );
        for part in 0..PROPERTY_PARTS {
            sql += &format!(
                "INSERT INTO event_properties_{part} SELECT u.* FROM event_properties_unpartitioned u
                 SEMI JOIN (SELECT key FROM property_keys WHERE part = {part}) k ON k.key = u.key
                 ORDER BY u.seq;"
            );
        }
        sql += "DROP TABLE event_properties_unpartitioned; COMMIT;";
        conn.execute_batch(&sql)?;
    }
    // A file whose derived tables predate the member event times, and with them the rollups
    // and the stored session fields: they are dropped and every day with events is rebuilt
    // by correction.
    let outdated: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM duckdb_tables() WHERE table_name = 'session_members')
            AND NOT EXISTS (SELECT 1 FROM duckdb_columns()
                            WHERE table_name = 'session_members' AND column_name = 'event_time')",
        [],
        |r| r.get(0),
    )?;
    if outdated {
        conn.execute_batch(
            "DROP TABLE sessions; DROP TABLE session_members; DROP TABLE day_aggregates;
             DROP TABLE IF EXISTS hour_aggregates; DROP TABLE IF EXISTS dimension_aggregates;
             DROP TABLE IF EXISTS visitor_days;",
        )?;
    }
    Ok(outdated)
}

/// The key dictionary, the property partitions and the view over them.
fn property_schema() -> String {
    let mut sql = String::from(
        "CREATE TABLE IF NOT EXISTS property_keys (key VARCHAR NOT NULL, part USMALLINT NOT NULL);\n",
    );
    let mut union = Vec::new();
    for part in 0..PROPERTY_PARTS {
        sql +=
            &format!("CREATE TABLE IF NOT EXISTS event_properties_{part} ({PROPERTY_COLUMNS});\n");
        union.push(format!("SELECT * FROM event_properties_{part}"));
    }
    sql += &format!(
        "CREATE VIEW IF NOT EXISTS event_properties AS {};",
        union.join(" UNION ALL ")
    );
    sql
}

/// The table holding the values of event property `key`: its partition, or the view over
/// every partition for a key no event carried.
pub fn event_property_table(conn: &Connection, key: &str) -> duckdb::Result<String> {
    let part: Option<u16> = conn
        .query_row("SELECT part FROM property_keys WHERE key = ?", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(part.map_or_else(
        || "event_properties".to_owned(),
        |part| format!("event_properties_{part}"),
    ))
}

/// The partition of every event property key in `records`. Keys stored for the first time
/// get the next partition round robin, in order of first appearance.
fn property_parts(
    conn: &Connection,
    records: &[&(i64, Record)],
) -> duckdb::Result<HashMap<String, u16>> {
    let mut parts: HashMap<String, u16> = conn
        .prepare("SELECT key, part FROM property_keys")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<duckdb::Result<_>>()?;
    for (_, record) in records.iter().copied() {
        let Record::Event(e) = record else { continue };
        for property in &e.properties {
            if !parts.contains_key(&property.key) {
                let part = (parts.len() % usize::from(PROPERTY_PARTS)) as u16;
                conn.execute(
                    "INSERT INTO property_keys VALUES (?, ?)",
                    params![property.key, part],
                )?;
                parts.insert(property.key.clone(), part);
            }
        }
    }
    Ok(parts)
}

pub(crate) fn watermark(conn: &Connection) -> duckdb::Result<Watermark> {
    conn.query_row(
        "SELECT batch_id, last_seq FROM applied_batches ORDER BY batch_id DESC LIMIT 1",
        [],
        |row| {
            Ok(Watermark {
                batch: row.get(0)?,
                sequence: row.get(1)?,
            })
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
}

/// Applies a stable batch exactly once: records, dirty days and the ledger entry
/// commit in one DuckDB transaction. A batch already in the ledger is not reinserted.
/// Records an erasure fence covers are left out; the ledger still records the whole batch.
pub(crate) fn apply(
    conn: &mut Connection,
    batch: &Batch,
    fences: &Fences,
) -> Result<Watermark, Error> {
    let tx = conn.transaction()?;
    let existing = tx
        .query_row(
            "SELECT first_seq, last_seq, records, analytics_schema, inbox_format FROM applied_batches WHERE batch_id = ?",
            [batch.id],
            |row| Ok([row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?]),
        )
        .optional()?;
    let membership = [
        batch.first_seq,
        batch.last_seq,
        batch.records.len() as i64,
        ANALYTICS_SCHEMA,
        INBOX_FORMAT,
    ];
    if let Some(entry) = existing {
        if entry != membership {
            return Err(Error::Incompatible(format!(
                "batch {} ledger entry {entry:?} differs from inbox membership {membership:?}",
                batch.id
            )));
        }
        drop(tx);
        return Ok(watermark(conn)?);
    }
    let previous = watermark(&tx)?;
    if batch.id <= previous.batch {
        return Err(Error::Incompatible(format!(
            "batch {} is older than applied batch {} but missing from the ledger",
            batch.id, previous.batch
        )));
    }

    let kept: Vec<&(i64, Record)> = batch
        .records
        .iter()
        .filter(|(seq, record)| !fences.blocks_record(*seq, record))
        .collect();
    append_records(&tx, &kept)?;
    failpoint::hit("transfer.during_apply");

    let mut dirty = tx.appender("dirty_days")?;
    for (site, day) in touched_days(&kept) {
        dirty.append_row(params![site, Value::Date32(day), batch.id])?;
    }
    dirty.flush()?;
    drop(dirty);

    tx.execute(
        "INSERT INTO applied_batches (batch_id, first_seq, last_seq, records, analytics_schema, inbox_format) VALUES (?, ?, ?, ?, ?, ?)",
        params![batch.id, batch.first_seq, batch.last_seq, batch.records.len() as i64, ANALYTICS_SCHEMA, INBOX_FORMAT],
    )?;
    tx.commit()?;
    Ok(Watermark {
        batch: batch.id,
        sequence: batch.last_seq,
    })
}

/// Identity days whose derived data a batch can change. A revocation can split
/// a profile session within one window (at most 24 h) of its instant, whose
/// local dates lie within two days of its UTC date.
fn touched_days(records: &[&(i64, Record)]) -> BTreeSet<(u128, i32)> {
    const DAY_MS: i64 = 86_400_000;
    let mut days = BTreeSet::new();
    for (_, record) in records.iter().copied() {
        match record {
            Record::Event(e) => {
                days.insert((e.site, e.local_day));
            }
            Record::Identify(i) => {
                days.insert((i.site, i.local_day));
            }
            Record::Revocation(r) => {
                let utc_day = r.revoked_at_ms.div_euclid(DAY_MS) as i32;
                days.extend((utc_day - 2..=utc_day + 2).map(|day| (r.site, day)));
            }
            Record::ReplayFragment(_) | Record::HeatmapInteraction(_) => {}
        }
    }
    days
}

fn ts(ms: i64) -> Value {
    Value::Timestamp(TimeUnit::Millisecond, ms)
}

fn append_records(conn: &Connection, records: &[&(i64, Record)]) -> duckdb::Result<()> {
    let parts = property_parts(conn, records)?;
    let mut events = conn.appender("events")?;
    let mut properties = HashMap::new();
    let mut session_properties = conn.appender("session_properties")?;
    let mut fragments = conn.appender("replay_fragments")?;
    let mut heatmap = conn.appender("heatmap_interactions")?;
    let mut revocations = conn.appender("consent_revocations")?;

    for (seq, record) in records.iter().copied() {
        match record {
            Record::Event(e) => {
                let d = &e.dimensions;
                let perf = e.performance.unwrap_or_default();
                let (currency, minor) = match &e.revenue {
                    Some(r) => (Some(r.currency.as_str()), Some(r.minor_units)),
                    None => (None, None),
                };
                let row: [&dyn ToSql; 40] = [
                    seq,
                    &e.id,
                    &e.site,
                    &(e.kind as u8),
                    &ts(e.event_time_ms),
                    &ts(e.receive_time_ms),
                    &Value::Date32(e.local_day),
                    &e.daily_id,
                    &e.profile_id,
                    &d.hostname,
                    &d.path,
                    &d.query,
                    &d.title,
                    &d.referrer_domain,
                    &d.referrer_path,
                    &d.utm_source,
                    &d.utm_medium,
                    &d.utm_campaign,
                    &d.utm_content,
                    &d.utm_term,
                    &d.click_id,
                    &d.country,
                    &d.region,
                    &d.city,
                    &d.browser,
                    &d.os,
                    &d.device,
                    &d.screen,
                    &d.language,
                    &d.event_name,
                    &d.tag,
                    &perf.lcp,
                    &perf.fcp,
                    &perf.ttfb,
                    &perf.inp,
                    &perf.cls,
                    &currency,
                    &minor,
                    &d.latitude,
                    &d.longitude,
                ];
                events.append_row(&row[..])?;
                let time = ts(e.event_time_ms);
                for property in &e.properties {
                    let typed = TypedValue::from(property);
                    let part = parts[&property.key];
                    let properties = match properties.entry(part) {
                        Entry::Occupied(entry) => entry.into_mut(),
                        Entry::Vacant(entry) => {
                            entry.insert(conn.appender(&format!("event_properties_{part}"))?)
                        }
                    };
                    properties.append_row(params![
                        seq,
                        e.site,
                        time,
                        property.key,
                        typed.tag,
                        typed.string,
                        typed.integer,
                        typed.decimal,
                        typed.boolean,
                        typed.date,
                        typed.array
                    ])?;
                }
            }
            Record::Identify(i) => {
                let time = ts(i.event_time_ms);
                for property in &i.properties {
                    let typed = TypedValue::from(property);
                    session_properties.append_row(params![
                        seq,
                        i.id,
                        i.site,
                        time,
                        Value::Date32(i.local_day),
                        i.daily_id,
                        i.profile_id,
                        property.key,
                        typed.tag,
                        typed.string,
                        typed.integer,
                        typed.decimal,
                        typed.boolean,
                        typed.date,
                        typed.array
                    ])?;
                }
            }
            Record::ReplayFragment(f) => fragments.append_row(params![
                seq,
                f.id,
                f.site,
                f.capture_id,
                f.index,
                f.total,
                f.is_final,
                ts(f.event_time_ms),
                ts(f.receive_time_ms),
                f.payload.as_slice()
            ])?,
            Record::HeatmapInteraction(h) => {
                let (kind, x, page_y, percent) = match h.kind {
                    Interaction::Click { x, page_y } => (1u8, Some(x), Some(page_y), None),
                    Interaction::Scroll { percent } => (2u8, None, None, Some(percent)),
                };
                heatmap.append_row(params![
                    seq,
                    h.id,
                    h.site,
                    h.capture_id,
                    ts(h.event_time_ms),
                    ts(h.receive_time_ms),
                    h.path,
                    h.viewport_width,
                    h.viewport_height,
                    kind,
                    x,
                    page_y,
                    percent
                ])?;
            }
            Record::Revocation(r) => {
                revocations.append_row(params![seq, r.id, r.site, r.profile_id, r.revoked_at_ms])?
            }
        }
    }
    for appender in [
        &mut events,
        &mut session_properties,
        &mut fragments,
        &mut heatmap,
        &mut revocations,
    ]
    .into_iter()
    .chain(properties.values_mut())
    {
        appender.flush()?;
    }
    Ok(())
}

/// Tables written by transfer, with `site` and the inbox `seq`. Event properties are the
/// partitions besides these.
const CANONICAL: [&str; 5] = [
    "events",
    "session_properties",
    "replay_fragments",
    "heatmap_interactions",
    "consent_revocations",
];
/// Tables correction rebuilds from the canonical ones, with `site`.
const DERIVED: [&str; 7] = [
    "sessions",
    "session_members",
    "day_aggregates",
    "hour_aggregates",
    "dimension_aggregates",
    "visitor_days",
    "dirty_days",
];
const LATEST_BATCH: &str = "(SELECT coalesce(max(batch_id), 0) FROM applied_batches)";

/// What an erasure removed from analytics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Erased {
    pub events: u64,
    /// Daily pseudonyms of a profile's erased events: the replays recorded under them go too.
    pub daily_ids: Vec<Id>,
}

fn property_tables() -> impl Iterator<Item = String> {
    (0..PROPERTY_PARTS).map(|part| format!("event_properties_{part}"))
}

/// Removes what `fence` covers from the canonical tables and the derived data built on them.
/// Days that keep other records are marked dirty, so correction rebuilds them. Running it
/// again removes nothing more.
pub(crate) fn erase(conn: &Connection, fence: &Fence) -> duckdb::Result<Erased> {
    let through = fence.through_seq;
    match fence.erasure {
        Erasure::Site(site) => {
            let events: u64 =
                conn.query_row("SELECT count(*) FROM events WHERE site = ?", [site], |r| {
                    r.get(0)
                })?;
            let tables = CANONICAL
                .iter()
                .chain(&DERIVED)
                .chain(&["session_policies", "import_provenance", "imported_history"])
                .map(|t| t.to_string())
                .chain(property_tables());
            for table in tables {
                conn.execute(&format!("DELETE FROM {table} WHERE site = ?"), [site])?;
            }
            Ok(Erased {
                events,
                daily_ids: Vec::new(),
            })
        }
        Erasure::SiteHistory(site) => {
            let events: u64 = conn.query_row(
                "SELECT count(*) FROM events WHERE site = ? AND seq <= ?",
                params![site, through],
                |r| r.get(0),
            )?;
            for table in CANONICAL
                .iter()
                .map(|t| t.to_string())
                .chain(property_tables())
            {
                conn.execute(
                    &format!("DELETE FROM {table} WHERE site = ? AND seq <= ?"),
                    params![site, through],
                )?;
            }
            // Imports refuse while serving, so every imported event is within `through`.
            for table in DERIVED
                .iter()
                .chain(&["import_provenance", "imported_history"])
            {
                conn.execute(&format!("DELETE FROM {table} WHERE site = ?"), [site])?;
            }
            conn.execute(
                &format!(
                    "INSERT INTO dirty_days
                     SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM events WHERE site = ?
                     UNION SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM session_properties WHERE site = ?"
                ),
                params![site, site],
            )?;
            Ok(Erased {
                events,
                daily_ids: Vec::new(),
            })
        }
        Erasure::Profile { site, profile } => {
            let scope = params![site, profile, through];
            conn.execute(
                "CREATE OR REPLACE TEMP TABLE erased AS
                 SELECT site, seq, local_day, daily_id FROM events
                 WHERE site = ? AND profile_id = ? AND seq <= ?",
                scope,
            )?;
            let events: u64 = conn.query_row("SELECT count(*) FROM erased", [], |r| r.get(0))?;
            let daily_ids = conn
                .prepare("SELECT DISTINCT daily_id FROM erased WHERE daily_id IS NOT NULL")?
                .query_map([], |r| r.get(0))?
                .collect::<duckdb::Result<Vec<Id>>>()?;
            for table in property_tables() {
                conn.execute(
                    &format!(
                        "DELETE FROM {table} WHERE site = ? AND seq IN (SELECT seq FROM erased)"
                    ),
                    [site],
                )?;
            }
            conn.execute(
                &format!(
                    "INSERT INTO dirty_days
                     SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM erased
                     UNION SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM session_properties
                     WHERE site = ? AND profile_id = ? AND seq <= ?"
                ),
                scope,
            )?;
            for table in ["events", "session_properties", "consent_revocations"] {
                conn.execute(
                    &format!("DELETE FROM {table} WHERE site = ? AND profile_id = ? AND seq <= ?"),
                    scope,
                )?;
            }
            // Correction finds a profile's sessions through its events, which are gone now.
            conn.execute(
                "DELETE FROM session_members WHERE site = ? AND session_id IN (
                     SELECT session_id FROM sessions WHERE site = ? AND population = ? AND identity = ?)",
                params![site, site, PROFILE_POPULATION, profile],
            )?;
            conn.execute(
                "DELETE FROM sessions WHERE site = ? AND population = ? AND identity = ?",
                params![site, PROFILE_POPULATION, profile],
            )?;
            // Later events of the profile get their sessions back.
            conn.execute(
                &format!(
                    "INSERT INTO dirty_days
                     SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM events
                     WHERE site = ? AND profile_id = ?"
                ),
                params![site, profile],
            )?;
            conn.execute_batch("DROP TABLE erased")?;
            Ok(Erased { events, daily_ids })
        }
    }
}

/// Removes a site's analytical rows from before its retention cutoff (0006): events and
/// their properties, identifies and revocations before `cutoff_ms`, sessions that start
/// before it, and day rows before `cutoff_day`, the site-local date the cutoff falls on.
/// A session crossing the cutoff loses its early members, so the days after the cutoff are
/// marked dirty and correction rebuilds it from the rest. Returns the events removed.
pub(crate) fn expire(
    conn: &Connection,
    site: Id,
    cutoff_ms: i64,
    cutoff_day: i32,
) -> duckdb::Result<u64> {
    const SESSION_REACH_MS: i64 = 2 * 86_400_000;
    let at = ts(cutoff_ms);
    let removed = conn.execute(
        "DELETE FROM events WHERE site = ? AND event_time < ?",
        params![site, at],
    )?;
    for table in property_tables().chain(["session_properties".to_owned()]) {
        conn.execute(
            &format!("DELETE FROM {table} WHERE site = ? AND event_time < ?"),
            params![site, at],
        )?;
    }
    conn.execute(
        "DELETE FROM consent_revocations WHERE site = ? AND revoked_at_ms < ?",
        params![site, cutoff_ms],
    )?;
    conn.execute(
        "DELETE FROM import_provenance WHERE site = ?
         AND event_id NOT IN (SELECT id FROM events WHERE site = ?)",
        params![site, site],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO dirty_days
             SELECT DISTINCT site, local_day, {LATEST_BATCH} FROM events
             WHERE site = ? AND event_time >= ? AND event_time < ?
               AND EXISTS (SELECT 1 FROM sessions WHERE site = ? AND start_time < ? AND last_time >= ?)"
        ),
        params![site, at, ts(cutoff_ms + SESSION_REACH_MS), site, at, at],
    )?;
    conn.execute(
        "DELETE FROM session_members WHERE site = ? AND session_id IN (
             SELECT session_id FROM sessions WHERE site = ? AND start_time < ?)",
        params![site, site, at],
    )?;
    conn.execute(
        "DELETE FROM sessions WHERE site = ? AND start_time < ?",
        params![site, at],
    )?;
    for table in [
        "day_aggregates",
        "hour_aggregates",
        "dimension_aggregates",
        "visitor_days",
        "dirty_days",
    ] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE site = ? AND local_day < ?"),
            params![site, Value::Date32(cutoff_day)],
        )?;
    }
    Ok(removed as u64)
}

/// A property value spread over the typed columns.
struct TypedValue {
    tag: u8,
    string: Option<String>,
    integer: Option<i64>,
    decimal: Option<i64>,
    boolean: Option<bool>,
    date: Option<Value>,
    array: Option<String>,
}

impl From<&Property> for TypedValue {
    fn from(property: &Property) -> Self {
        let mut v = TypedValue {
            tag: 0,
            string: None,
            integer: None,
            decimal: None,
            boolean: None,
            date: None,
            array: None,
        };
        match &property.value {
            PropertyValue::Null => v.tag = property_type::NULL,
            PropertyValue::Bool(b) => (v.tag, v.boolean) = (property_type::BOOL, Some(*b)),
            PropertyValue::Integer(i) => (v.tag, v.integer) = (property_type::INTEGER, Some(*i)),
            PropertyValue::Decimal(d) => (v.tag, v.decimal) = (property_type::DECIMAL, Some(*d)),
            PropertyValue::Date(ms) => (v.tag, v.date) = (property_type::DATE, Some(ts(*ms))),
            PropertyValue::String(s) => {
                (v.tag, v.string) = (property_type::STRING, Some(s.clone()))
            }
            PropertyValue::Array(items) => {
                (v.tag, v.array) = (property_type::ARRAY, Some(array_json(items)))
            }
        }
        v
    }
}

/// Arrays are stored as JSON text of typed scalars. Milestone 06 decides the
/// queryable array representation; the workload has no array predicate in the early gate.
fn array_json(items: &[PropertyValue]) -> String {
    let values: Vec<serde_json::Value> = items
        .iter()
        .map(|item| match item {
            PropertyValue::Null => serde_json::Value::Null,
            PropertyValue::Bool(b) => (*b).into(),
            PropertyValue::Integer(i) => (*i).into(),
            PropertyValue::Decimal(d) => {
                let sign = if *d < 0 { "-" } else { "" };
                format!("{sign}{}.{:04}", d.abs() / 10_000, d.abs() % 10_000).into()
            }
            PropertyValue::Date(ms) => serde_json::json!({ "date_ms": ms }),
            PropertyValue::String(s) => s.as_str().into(),
            PropertyValue::Array(_) => {
                unreachable!("collection serializes nested arrays to strings")
            }
        })
        .collect();
    serde_json::Value::Array(values).to_string()
}
