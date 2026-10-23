//! Heatmaps (CAP-RECORD-05). Interactions travel through the durable inbox into DuckDB's
//! `heatmap_interactions`, then fold into per-day cells once their capture has settled. Reads
//! add the interactions not folded yet, so a heatmap is current at the analytical watermark.
//!
//! Cells: clicks by viewport class, one of 64 columns of the viewport width and 32-pixel rows of
//! the page; scroll depth as the deepest decile each page visit reached. Days are UTC dates.

use holocron_protocol::operations::recordings::{
    HeatmapCell, HeatmapCells, HeatmapPage, ScrollDepth, ViewportClass,
};
use holocron_storage::duckdb::types::Value;
use holocron_storage::duckdb::{self, Connection, params};
use holocron_storage::{HeatmapInteraction, Interaction, Record, Storage};
use uuid::Uuid;

use super::policy;
use super::replay::path_of;
use super::wire::{HeatmapEvent, HeatmapKind};

pub const COLUMNS: u16 = 64;
pub const ROW_HEIGHT: u16 = 32;
const MAX_ROW: u32 = 4095;
const MAX_CELLS: usize = 4096;
const MAX_PAGES: usize = 100;
const DAY_MS: i64 = 86_400_000;
/// Aggregated heatmaps are kept 90 days (0006).
pub const RETENTION_DAYS: i64 = 90;
/// A capture's interactions fold once none arrived for this long, so one page visit's scroll
/// depth is counted once.
pub const SETTLE_MS: i64 = 60 * 60 * 1000;
/// Browser times further off than this from receive time are replaced by receive time.
const CLOCK_SKEW_MS: i64 = DAY_MS;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS heatmap_cells (
    site UHUGEINT NOT NULL,
    day DATE NOT NULL,
    path VARCHAR NOT NULL,
    viewport UTINYINT NOT NULL,
    kind UTINYINT NOT NULL,
    cell_col USMALLINT NOT NULL,
    cell_row UINTEGER NOT NULL,
    n BIGINT NOT NULL,
    PRIMARY KEY (site, day, path, viewport, kind, cell_col, cell_row)
);";

/// SQL for the viewport class code of `heatmap_interactions.viewport_width`.
const VIEWPORT: &str =
    "CASE WHEN viewport_width < 768 THEN 1 WHEN viewport_width < 1280 THEN 2 ELSE 3 END";
const CLICK: u8 = 1;
const SCROLL: u8 = 2;

pub async fn create(storage: &Storage) -> Result<(), holocron_storage::Error> {
    storage.write(|tx| tx.execute_batch(SCHEMA)).await
}

fn viewport_code(viewport: ViewportClass) -> u8 {
    match viewport {
        ViewportClass::Mobile => 1,
        ViewportClass::Tablet => 2,
        ViewportClass::Desktop => 3,
    }
}

pub fn utc_day(ms: i64) -> i32 {
    ms.div_euclid(DAY_MS) as i32
}

/// The first UTC day still inside heatmap retention at `now_ms`.
pub fn first_retained_day(now_ms: i64) -> i32 {
    utc_day(now_ms) - RETENTION_DAYS as i32
}

/// Interaction records for one heatmap payload. Events on excluded pages, and events without
/// the coordinates a cell needs, are left out. Returns the records and how many were excluded.
pub fn records(
    site: u128,
    capture: u128,
    events: &[HeatmapEvent],
    excluded_paths: &[String],
    now_ms: i64,
) -> (Vec<Record>, usize) {
    let mut excluded = 0;
    let mut records = Vec::with_capacity(events.len());
    for event in events {
        let path = path_of(&event.url);
        if policy::excluded(excluded_paths, &path) {
            excluded += 1;
            continue;
        }
        let Some(width) = event.viewport_w.filter(|w| *w >= 1.0) else {
            continue;
        };
        let kind = match event.kind {
            HeatmapKind::Click => match (event.page_x, event.page_y) {
                (Some(x), Some(y)) => Interaction::Click {
                    x: x.clamp(0.0, f64::from(u16::MAX)) as u16,
                    page_y: y.clamp(0.0, f64::from(u32::MAX)) as u32,
                },
                _ => continue,
            },
            HeatmapKind::Scroll => match event.scroll_pct {
                Some(percent) => Interaction::Scroll {
                    percent: percent.clamp(0.0, 100.0) as u8,
                },
                None => continue,
            },
        };
        let event_time = event
            .timestamp
            .map(|t| t as i64)
            .filter(|t| (now_ms - CLOCK_SKEW_MS..=now_ms + 5 * 60_000).contains(t))
            .unwrap_or(now_ms);
        records.push(Record::HeatmapInteraction(HeatmapInteraction {
            id: Uuid::new_v4().as_u128(),
            site,
            capture_id: capture,
            event_time_ms: event_time,
            receive_time_ms: now_ms,
            path,
            viewport_width: width.min(f64::from(u16::MAX)) as u16,
            viewport_height: event
                .viewport_h
                .unwrap_or(0.0)
                .clamp(0.0, f64::from(u16::MAX)) as u16,
            kind,
        }));
    }
    (records, excluded)
}

/// Folds the interactions of up to `captures` settled captures into cells and deletes them, in
/// one transaction. Returns how many captures folded.
pub async fn fold(
    storage: &Storage,
    settled_before_ms: i64,
    captures: usize,
) -> Result<usize, holocron_storage::Error> {
    storage
        .write(move |tx| {
            tx.execute(
                "CREATE OR REPLACE TEMP TABLE heatmap_fold AS
                 SELECT site, capture_id FROM heatmap_interactions
                 GROUP BY site, capture_id HAVING max(receive_time) < epoch_ms(?) LIMIT ?",
                params![settled_before_ms, captures as i64],
            )?;
            let folded: i64 = tx.query_row("SELECT count(*) FROM heatmap_fold", [], |r| r.get(0))?;
            if folded > 0 {
                tx.execute(
                    &format!(
                        "INSERT INTO heatmap_cells
                         SELECT h.site, CAST(h.event_time AS DATE), h.path, {VIEWPORT}, {CLICK},
                                least({max_col}, (h.x::INTEGER * {COLUMNS}) // h.viewport_width),
                                least({MAX_ROW}, h.page_y // {ROW_HEIGHT}), count(*)
                         FROM heatmap_interactions h JOIN heatmap_fold f USING (site, capture_id)
                         WHERE h.kind = {CLICK} AND h.viewport_width > 0
                         GROUP BY ALL
                         ON CONFLICT DO UPDATE SET n = n + excluded.n",
                        max_col = COLUMNS - 1
                    ),
                    [],
                )?;
                tx.execute(
                    &format!(
                        "INSERT INTO heatmap_cells
                         SELECT site, day, path, viewport, {SCROLL}, 0, least(10, depth // 10), count(*)
                         FROM (SELECT h.site, CAST(h.event_time AS DATE) AS day, h.path, {VIEWPORT} AS viewport,
                                      h.capture_id, max(h.scroll_percent) AS depth
                               FROM heatmap_interactions h JOIN heatmap_fold f USING (site, capture_id)
                               WHERE h.kind = {SCROLL} AND h.viewport_width > 0
                               GROUP BY ALL)
                         GROUP BY ALL
                         ON CONFLICT DO UPDATE SET n = n + excluded.n"
                    ),
                    [],
                )?;
                tx.execute(
                    "DELETE FROM heatmap_interactions h USING heatmap_fold f
                     WHERE h.site = f.site AND h.capture_id = f.capture_id",
                    [],
                )?;
            }
            tx.execute_batch("DROP TABLE heatmap_fold")?;
            Ok(folded as usize)
        })
        .await
}

/// Removes cells and interactions older than heatmap retention.
pub async fn expire(storage: &Storage, now_ms: i64) -> Result<(), holocron_storage::Error> {
    let first = first_retained_day(now_ms);
    storage
        .write(move |tx| {
            tx.execute(
                "DELETE FROM heatmap_cells WHERE day < ?",
                [Value::Date32(first)],
            )?;
            tx.execute(
                "DELETE FROM heatmap_interactions WHERE CAST(event_time AS DATE) < ?",
                [Value::Date32(first)],
            )?;
            Ok(())
        })
        .await
}

/// Pages with interactions on UTC days `first..=last`, most clicked first.
pub fn pages(
    conn: &Connection,
    site: u128,
    first: i32,
    last: i32,
) -> duckdb::Result<Vec<HeatmapPage>> {
    let mut statement = conn.prepare(&format!(
        "SELECT path, sum(clicks)::BIGINT, sum(visits)::BIGINT FROM (
             SELECT path, sum(n) FILTER (kind = {CLICK}) AS clicks, sum(n) FILTER (kind = {SCROLL}) AS visits
             FROM heatmap_cells WHERE site = ?1 AND day BETWEEN ?2 AND ?3 GROUP BY path
             UNION ALL
             SELECT path, count(*) FILTER (kind = {CLICK}), count(DISTINCT capture_id) FILTER (kind = {SCROLL})
             FROM heatmap_interactions WHERE site = ?1 AND CAST(event_time AS DATE) BETWEEN ?2 AND ?3 GROUP BY path)
         GROUP BY path ORDER BY 2 DESC NULLS LAST, 3 DESC NULLS LAST, path LIMIT {MAX_PAGES}"
    ))?;
    let rows = statement.query_map(
        params![site, Value::Date32(first), Value::Date32(last)],
        |row| {
            Ok(HeatmapPage {
                path: row.get(0)?,
                clicks: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                scroll_visits: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            })
        },
    )?;
    rows.collect()
}

/// Click cells and scroll depth of one page and viewport class on UTC days `first..=last`.
pub fn cells(
    conn: &Connection,
    site: u128,
    path: &str,
    viewport: ViewportClass,
    first: i32,
    last: i32,
) -> duckdb::Result<HeatmapCells> {
    let code = viewport_code(viewport);
    let args = params![site, path, code, Value::Date32(first), Value::Date32(last)];
    let clicks_sql = format!(
        "SELECT cell_col, cell_row, sum(n)::BIGINT AS n FROM (
             SELECT cell_col, cell_row, n FROM heatmap_cells
             WHERE site = ?1 AND path = ?2 AND viewport = ?3 AND kind = {CLICK} AND day BETWEEN ?4 AND ?5
             UNION ALL
             SELECT least({max_col}, (x::INTEGER * {COLUMNS}) // viewport_width)::USMALLINT,
                    least({MAX_ROW}, page_y // {ROW_HEIGHT})::UINTEGER, count(*)
             FROM heatmap_interactions
             WHERE site = ?1 AND path = ?2 AND {VIEWPORT} = ?3 AND kind = {CLICK} AND viewport_width > 0
               AND CAST(event_time AS DATE) BETWEEN ?4 AND ?5
             GROUP BY ALL)
         GROUP BY ALL",
        max_col = COLUMNS - 1
    );
    let total_clicks: i64 = conn.query_row(
        &format!("SELECT coalesce(sum(n), 0)::BIGINT FROM ({clicks_sql})"),
        args,
        |row| row.get(0),
    )?;
    let mut statement = conn.prepare(&format!(
        "{clicks_sql} ORDER BY n DESC, cell_row, cell_col LIMIT {MAX_CELLS}"
    ))?;
    let clicks = statement
        .query_map(args, |row| {
            Ok(HeatmapCell {
                column: row.get(0)?,
                row: row.get(1)?,
                count: row.get(2)?,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    let mut deciles = [0i64; 11];
    let mut statement = conn.prepare(&format!(
        "SELECT cell_row, sum(n)::BIGINT FROM (
             SELECT cell_row, n FROM heatmap_cells
             WHERE site = ?1 AND path = ?2 AND viewport = ?3 AND kind = {SCROLL} AND day BETWEEN ?4 AND ?5
             UNION ALL
             SELECT least(10, depth // 10)::UINTEGER, count(*) FROM (
                 SELECT capture_id, max(scroll_percent) AS depth FROM heatmap_interactions
                 WHERE site = ?1 AND path = ?2 AND {VIEWPORT} = ?3 AND kind = {SCROLL} AND viewport_width > 0
                   AND CAST(event_time AS DATE) BETWEEN ?4 AND ?5
                 GROUP BY capture_id, CAST(event_time AS DATE))
             GROUP BY ALL)
         GROUP BY ALL"
    ))?;
    for row in statement.query_map(args, |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (decile, visits) = row?;
        deciles[decile.min(10) as usize] += visits;
    }
    // A visit that reached 70 % also reached every shallower depth.
    let mut reached = 0;
    let mut scroll: Vec<ScrollDepth> = deciles
        .iter()
        .enumerate()
        .rev()
        .map(|(decile, visits)| {
            reached += visits;
            ScrollDepth {
                depth_percent: decile as u8 * 10,
                visits: reached,
            }
        })
        .collect();
    scroll.reverse();
    Ok(HeatmapCells {
        columns: COLUMNS,
        row_height: ROW_HEIGHT,
        clicks,
        total_clicks,
        scroll_visits: reached,
        scroll,
    })
}
