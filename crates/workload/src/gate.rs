//! Gate configuration, session derivation and inventory checks.

use std::time::Duration;

use holocron_analytics::identity::{DEFAULT_WINDOW_MINUTES, SessionPolicy, SitePolicies};
use holocron_analytics::jobs::record_policies;
use holocron_storage::{Budgets, Storage};
use serde_json::Value as Json;

use crate::catalog::Catalog;

/// Budgets for every measured run (decision 0008). Reads them from the
/// environment only to let the evidence report record alternative runs.
pub fn budgets() -> Budgets {
    let mut b = Budgets::default();
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.parse::<u64>().expect(name))
    };
    if let Some(mib) = var("HOLOCRON_ENGINE_MEMORY_MIB") {
        b.engine_memory_bytes = mib << 20;
    }
    if let Some(threads) = var("HOLOCRON_ENGINE_THREADS") {
        b.engine_threads = threads as u32;
    }
    if let Some(ms) = var("HOLOCRON_TRANSFER_DELAY_MS") {
        b.transfer_delay = Duration::from_millis(ms);
    }
    if let Some(mib) = var("HOLOCRON_DISK_RESERVE_MIB") {
        b.disk_reserve_bytes = mib << 20;
    }
    if let Some(s) = var("HOLOCRON_QUERY_TIMEOUT_S") {
        b.query_timeout = Duration::from_secs(s);
    }
    b
}

/// Bulk load keeps the same durability path with larger batches and backlog.
pub fn load_budgets() -> Budgets {
    Budgets {
        inbox_bytes: 4 << 30,
        admission_queue_bytes: 256 << 20,
        batch_records: 200_000,
        batch_bytes: 128 << 20,
        transfer_delay: Duration::from_millis(50),
        ..budgets()
    }
}

/// Records each site's session policy as the server does at startup, then derives sessions,
/// members, day aggregates and rollups for every dirty identity day (milestone 05
/// correction). Returns the number of sessions written.
pub async fn derive_sessions(
    storage: &Storage,
    catalog: &Catalog,
) -> Result<u64, holocron_storage::Error> {
    for site in &catalog.sites {
        let policy = SessionPolicy::new(site.timezone, DEFAULT_WINDOW_MINUTES).expect("policy");
        record_policies(storage, site.id.as_u128(), SitePolicies::new(policy)).await?;
    }
    let done =
        holocron_analytics::run_until_clean(storage, holocron_analytics::Limits::default()).await?;
    Ok(done.sessions)
}

/// Events per compaction step, as `apps/server/src/maintenance/compaction.rs` moves them.
const COMPACTION_STEP_EVENTS: u64 = 250_000;
/// A month counts as closed one day after it ended, as in the server.
const COMPACTION_SETTLE_MS: i64 = 86_400_000;

/// Clusters every month closed at `now_ms` by site through the production path, one step
/// after another without the server's pauses (0014 Compaction). Returns the events moved.
pub async fn compact(storage: &Storage, now_ms: i64) -> Result<u64, holocron_storage::Error> {
    let mut moved = 0;
    while let Some(step) = storage
        .compact(now_ms - COMPACTION_SETTLE_MS, COMPACTION_STEP_EVENTS)
        .await?
    {
        moved += step.events;
    }
    Ok(moved)
}

/// Layout experiment on the analytical writer: rewrites a table month by
/// month, each month ordered by (site, event_time), so zone maps let a
/// one-site query skip other sites' row groups. Live ingestion appends in
/// arrival order, so a production layout needs this as a periodic compaction
/// of closed periods. One transaction per month keeps sort spill bounded.
pub async fn recluster(storage: &Storage, table: String) -> Result<(), holocron_storage::Error> {
    let name = table.clone();
    storage
        .write(move |tx| {
            let table = name;
            tx.execute_batch(&format!(
                "DROP TABLE IF EXISTS {table}_clustered; CREATE TABLE {table}_clustered AS SELECT * FROM {table} LIMIT 0;"
            ))
        })
        .await?;
    for month in 0..13 {
        // The first window starts a month early: identify messages precede their group by 1 s.
        let from = if month == 0 {
            "2025-09-01".to_owned()
        } else {
            crate::fixture::month_start(month).to_string()
        };
        let to = crate::fixture::month_start(month + 1);
        let table = table.clone();
        storage
            .write(move |tx| {
                tx.execute_batch(&format!(
                    "SET preserve_insertion_order = true;
                     INSERT INTO {table}_clustered SELECT * FROM {table}
                     WHERE event_time >= TIMESTAMP '{from}' AND event_time < TIMESTAMP '{to}' ORDER BY site, event_time;
                     SET preserve_insertion_order = false;"
                ))
            })
            .await?;
    }
    storage
        .write(move |tx| {
            let before: i64 =
                tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            let after: i64 = tx.query_row(
                &format!("SELECT count(*) FROM {table}_clustered"),
                [],
                |r| r.get(0),
            )?;
            assert_eq!(
                before, after,
                "{table}: every row lies in one of the 13 months"
            );
            tx.execute_batch(&format!(
                "DROP TABLE {table}; ALTER TABLE {table}_clustered RENAME TO {table};"
            ))
        })
        .await
}

pub fn selectivity_class(matched: u64, population: u64) -> &'static str {
    let share = matched as f64 / population.max(1) as f64;
    match share {
        s if s <= 0.001 => "selective",
        s if (0.01..=0.10).contains(&s) => "medium",
        s if s >= 0.25 => "broad",
        _ => "between",
    }
}

pub fn fits_class(declared: &str, matched: u64, population: u64) -> bool {
    selectivity_class(matched, population) == declared
}

/// Fails loudly when generated totals disagree with the frozen manifest arithmetic.
pub fn check_inventory(cat: &Catalog, inventory: &Json) {
    let m = &cat.manifest;
    let fixture = &m["retained_fixture"];
    let mut failures = Vec::new();
    let mut expect = |what: &str, got: &Json, want: &Json| {
        if got != want {
            failures.push(format!("{what}: generated {got}, manifest {want}"));
        }
    };
    expect(
        "primary events",
        &inventory["primary_events"],
        &fixture["primary_events"],
    );
    for kind in ["pageview", "custom_event", "performance"] {
        let generated = match kind {
            "custom_event" => &inventory["by_kind"]["custom"],
            k => &inventory["by_kind"][k],
        };
        expect(kind, generated, &fixture["by_kind"][kind]);
    }
    for site in m["sites"].as_array().unwrap() {
        let code = site["code"].as_str().unwrap();
        expect(
            &format!("site {code}"),
            &inventory["by_site"][code],
            &site["primary_events"],
        );
    }
    expect(
        "identified groups",
        &inventory["identified_groups"],
        &fixture["identity"]["identified_groups"],
    );
    expect(
        "records with properties",
        &inventory["records_with_properties"],
        &fixture["event_properties"]["records_with_properties"],
    );
    expect(
        "property values",
        &inventory["property_values"],
        &fixture["event_properties"]["property_values"],
    );
    expect(
        "session property values",
        &inventory["session_property_values"],
        &fixture["session_properties"]["values"],
    );
    for (code, sums) in fixture["revenue"]["expected_sums"].as_object().unwrap() {
        let got = &inventory["revenue"][code];
        for field in [
            "gross_minor",
            "refunds_minor",
            "net_minor",
            "records",
            "refund_records",
        ] {
            expect(
                &format!("revenue {code} {field}"),
                &got[field],
                &sums[field],
            );
        }
    }
    for month in fixture["months"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["retained_fixture"] == true)
    {
        let index = month["month_index"].as_u64().unwrap().to_string();
        expect(
            &format!("groups per local day, month {index}"),
            &inventory["groups_per_local_day_by_month"][&index],
            &month["groups_per_local_day"],
        );
    }
    if !failures.is_empty() {
        panic!(
            "generated fixture disagrees with the manifest:\n{}",
            failures.join("\n")
        );
    }
    eprintln!("inventory: every manifest total matches");
}

/// File sizes, row counts and per-column compression of the analytical store.
pub async fn layout(storage: &Storage, data: &std::path::Path) -> Json {
    let files: serde_json::Map<String, Json> = std::fs::read_dir(data)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            Some((
                e.file_name().into_string().ok()?,
                serde_json::json!(e.metadata().ok()?.len()),
            ))
        })
        .collect();
    let read = storage
        .read(holocron_storage::Lane::Live, |snap| {
            let conn = snap.connection();
            let mut tables = serde_json::Map::new();
            let names: Vec<String> = {
                let mut st = conn.prepare("SELECT table_name FROM duckdb_tables() ORDER BY table_name")?;
                st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
            };
            for table in names {
                let rows: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
                let mut st = conn.prepare(&format!(
                    "SELECT column_name, string_agg(DISTINCT compression, ','), count(DISTINCT block_id), count(*)
                     FROM pragma_storage_info('{table}') WHERE segment_type <> 'VALIDITY' GROUP BY column_name, column_id ORDER BY column_id"
                ))?;
                let columns: Vec<Json> = st
                    .query_map([], |r| {
                        Ok(serde_json::json!({
                            "column": r.get::<_, String>(0)?,
                            "compression": r.get::<_, String>(1)?,
                            "blocks": r.get::<_, i64>(2)?,
                            "segments": r.get::<_, i64>(3)?,
                        }))
                    })?
                    .collect::<Result<_, _>>()?;
                let blocks: i64 = conn.query_row(
                    &format!("SELECT count(DISTINCT block_id) FROM pragma_storage_info('{table}')"),
                    [],
                    |r| r.get(0),
                )?;
                tables.insert(table, serde_json::json!({ "rows": rows, "distinct_blocks": blocks, "columns": columns }));
            }
            let size: Json = conn.query_row(
                "SELECT block_size, total_blocks, used_blocks, free_blocks, wal_size, memory_usage, memory_limit FROM pragma_database_size()",
                [],
                |r| {
                    Ok(serde_json::json!({
                        "block_size": r.get::<_, i64>(0)?,
                        "total_blocks": r.get::<_, i64>(1)?,
                        "used_blocks": r.get::<_, i64>(2)?,
                        "free_blocks": r.get::<_, i64>(3)?,
                        "wal_size": r.get::<_, String>(4)?,
                        "memory_usage": r.get::<_, String>(5)?,
                        "memory_limit": r.get::<_, String>(6)?,
                    }))
                },
            )?;
            let version: String = conn.query_row("SELECT version()", [], |r| r.get(0))?;
            Ok(serde_json::json!({ "duckdb_version": version, "database_size": size, "tables": tables }))
        })
        .await
        .expect("layout read");
    let mut out = read.value;
    out["files"] = Json::Object(files);
    out
}
