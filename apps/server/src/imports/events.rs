//! Timestamped facts in chunks: events with their properties and revenue, session data as
//! identify records, heatmap interactions, and session links kept in the inventory.
//!
//! A chunk is up to `chunk_rows` source rows after the step's cursor, in primary key order. Its
//! records enter the inbox under the key `<step>:<n>` together with that key's ledger row, then
//! the checkpoint moves the cursor. A crash between the two repeats the same chunk under the same
//! key, which the inbox recognizes, so nothing is admitted twice. Transfer then applies the
//! records like collected ones, marks their identity days dirty, and correction recomputes
//! sessions from event times.

use std::collections::HashMap;

use holocron_analytics::identity::{SessionPolicy, SitePolicies};
use holocron_storage::duckdb::params;
use holocron_storage::{
    Dimensions, Event, EventKind, HeatmapInteraction, Identify, Interaction, Performance, Record,
};
use serde_json::value::RawValue;
use sqlx::Row;
use sqlx::any::AnyRow;
use uuid::Uuid;

use super::Ctx;
use super::signature::{self, Engine, Layout};
use super::source::{self, Arg, Kind, RowExt};
use super::state::{self, Entry, Progress};
use crate::collect::decimal::Decimal;
use crate::collect::properties::{self, Notes};
use crate::init::Failure;
use crate::recordings::heatmaps;

pub const EVENTS: &str = "events";
pub const IDENTIFY: &str = "identify";
pub const HEATMAPS: &str = "heatmaps";
pub const SESSION_LINKS: &str = "session_links";

/// Sorts before every UUID in both engines.
const FIRST: &str = "00000000-0000-0000-0000-000000000000";

/// What an imported event may belong to, by source ID.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Site,
    Link,
    Pixel,
}

struct Targets {
    by_id: HashMap<u128, Target>,
    site: SitePolicies,
    utc: SitePolicies,
    site_cutoff: i64,
    utc_cutoff: i64,
    /// Length of each text column of an events chunk by position, so a source column wider
    /// than the layout's yields no longer values than Umami writes (0013).
    widths: Vec<Option<usize>>,
}

impl Targets {
    async fn load(ctx: &Ctx) -> Result<Self, Failure> {
        let mut by_id = HashMap::new();
        for (table, target) in [
            ("sites", Target::Site),
            ("links", Target::Link),
            ("pixels", Target::Pixel),
        ] {
            let ids: Vec<String> =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {table}")))
                    .fetch_all(&ctx.db.reader)
                    .await
                    .map_err(super::internal)?;
            for id in ids {
                by_id.insert(uuid(&id), target);
            }
        }
        let site =
            SitePolicies::new(SessionPolicy::new(ctx.timezone, 30).map_err(super::internal)?);
        let utc =
            SitePolicies::new(SessionPolicy::new(chrono_tz::UTC, 30).map_err(super::internal)?);
        let width = |table, column| signature::width(ctx.layout, table, column);
        let mut widths = vec![None; 6];
        widths.extend(EVENT_TEXT.map(|c| width("website_event", c)));
        widths.push(width("website_event", CLICK_IDS[0]));
        widths.extend(VITALS.map(|_| None));
        widths.extend(SESSION_TEXT.map(|c| width("session", c)));
        Ok(Self {
            widths,
            site_cutoff: site.retention_floor_ms(ctx.clock_ms, ctx.options.retention),
            utc_cutoff: utc.retention_floor_ms(ctx.clock_ms, ctx.options.retention),
            by_id,
            site,
            utc,
        })
    }

    /// Policies and retention cutoff of a target: the site timezone for sites, UTC for links
    /// and pixels (0001, 0006).
    fn policies(&self, target: Target) -> (&SitePolicies, i64) {
        match target {
            Target::Site => (&self.site, self.site_cutoff),
            Target::Link | Target::Pixel => (&self.utc, self.utc_cutoff),
        }
    }
}

pub fn uuid(text: &str) -> u128 {
    Uuid::parse_str(text).map_or(0, |u| u.as_u128())
}

pub async fn import(ctx: &mut Ctx) -> Result<(), Failure> {
    let targets = Targets::load(ctx).await?;
    events(ctx, &targets).await?;
    identify(ctx, &targets).await?;
    if ctx.layout == Layout::Umami3Postgresql {
        heatmap_events(ctx, &targets).await?;
        session_links(ctx).await?;
    }
    Ok(())
}

/// Commits one chunk: provenance (replaced by chunk), the records under the chunk's key, then
/// the checkpoint with any inventory entries in one control transaction.
struct Chunk {
    step: &'static str,
    number: i64,
    cursor: String,
    records: Vec<Record>,
    provenance: Vec<Provenance>,
    inventory: Vec<Entry>,
    progress: Progress,
    done: bool,
}

struct Provenance {
    event: u128,
    site: u128,
    session: u128,
    visit: Option<u128>,
    distinct_id: Option<String>,
}

async fn commit(ctx: &Ctx, chunk: Chunk) -> Result<(), Failure> {
    if !chunk.provenance.is_empty() {
        let number = chunk.number;
        let rows = chunk.provenance;
        ctx.storage
            .write(move |tx| {
                tx.execute("DELETE FROM import_provenance WHERE chunk = ?", [number])?;
                let mut appender = tx.appender("import_provenance")?;
                for p in rows {
                    appender.append_row(params![
                        number,
                        p.event,
                        p.site,
                        p.session,
                        p.visit,
                        p.distinct_id
                    ])?;
                }
                appender.flush()
            })
            .await
            .map_err(super::storage)?;
    }
    if !chunk.records.is_empty() {
        let key = format!("{}:{}", chunk.step, chunk.number);
        ctx.storage
            .admit_import(&key, &chunk.records)
            .await
            .map_err(super::storage)?;
    }
    let mut tx = ctx
        .db
        .writer
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(super::internal)?;
    state::record(&mut tx, &chunk.inventory)
        .await
        .map_err(super::internal)?;
    state::checkpoint(
        &mut tx,
        chunk.step,
        &chunk.cursor,
        chunk.number,
        chunk.progress,
        chunk.done,
    )
    .await
    .map_err(super::internal)?;
    tx.commit().await.map_err(super::internal)?;
    ctx.progress
        .checkpoint(chunk.step, chunk.progress.rows, chunk.done);
    Ok(())
}

fn text(row: &AnyRow, i: usize) -> Result<Option<String>, Failure> {
    row.try_get(i).map_err(super::source_error)
}

fn int(row: &AnyRow, i: usize) -> Result<Option<i64>, Failure> {
    row.try_get(i).map_err(super::source_error)
}

/// Text columns of `website_event` after the event type, in SELECT order. The click ID
/// follows them.
const EVENT_TEXT: [&str; 13] = [
    "url_path",
    "url_query",
    "referrer_path",
    "referrer_domain",
    "page_title",
    "event_name",
    "tag",
    "hostname",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_content",
    "utm_term",
];

/// Collection keeps the first click ID in this order. All share one length.
const CLICK_IDS: [&str; 6] = [
    "gclid",
    "fbclid",
    "msclkid",
    "ttclid",
    "li_fat_id",
    "twclid",
];

const VITALS: [&str; 5] = ["lcp", "fcp", "ttfb", "inp", "cls"];

/// Text columns of `session`, read after the web vitals.
const SESSION_TEXT: [&str; 9] = [
    "browser",
    "os",
    "device",
    "screen",
    "language",
    "country",
    "region",
    "city",
    "distinct_id",
];

/// The SELECT list of an events chunk: event columns, then the session's.
fn event_columns(engine: Engine, layout: Layout) -> String {
    let mut columns: Vec<String> = ["e.event_id", "e.website_id", "e.session_id", "e.visit_id"]
        .iter()
        .map(|c| engine.text(c))
        .collect();
    columns.push(engine.ms("e.created_at"));
    columns.push(engine.int("e.event_type"));
    for c in EVENT_TEXT {
        columns.push(engine.text(&format!("e.{c}")));
    }
    let click_ids: Vec<String> = CLICK_IDS.iter().map(|c| format!("e.{c}")).collect();
    columns.push(engine.text(&format!("coalesce({})", click_ids.join(", "))));
    for c in VITALS {
        columns.push(match layout {
            Layout::Umami3Postgresql => engine.text(&format!("e.{c}")),
            Layout::Umami2Mysql => "NULL".into(),
        });
    }
    for c in SESSION_TEXT {
        columns.push(engine.text(&format!("s.{c}")));
    }
    columns.join(", ")
}

async fn events(ctx: &mut Ctx, targets: &Targets) -> Result<(), Failure> {
    let mut step = state::step(&ctx.db, EVENTS)
        .await
        .map_err(super::internal)?;
    if step.done {
        return Ok(());
    }
    let engine = ctx.source.engine;
    let id = engine.id_param();
    let select = format!(
        "SELECT {} FROM website_event e LEFT JOIN session s ON s.session_id = e.session_id
         WHERE e.event_id > {id} ORDER BY e.event_id LIMIT ?",
        event_columns(engine, ctx.layout)
    );
    let data = format!(
        "SELECT {}, {}, {}, {}, {}, {} FROM event_data
         WHERE website_event_id > {id} AND website_event_id <= {id}
         ORDER BY website_event_id, event_data_id",
        engine.text("website_event_id"),
        engine.text("data_key"),
        engine.text("string_value"),
        engine.text("number_value"),
        engine.ms("date_value"),
        engine.int("data_type"),
    );
    let mut cursor = step.cursor.clone().unwrap_or_else(|| FIRST.into());
    loop {
        let rows = ctx
            .source
            .rows(
                &select,
                &[Arg::Text(&cursor), Arg::Int(ctx.options.chunk_rows)],
            )
            .await?;
        let number = step.chunks + 1;
        let Some(last) = rows.last() else {
            return commit(ctx, Chunk::empty(EVENTS, number, &cursor)).await;
        };
        let last = text(last, 0)?.unwrap_or_default();
        let mut data_rows: HashMap<String, Vec<DataRow>> = HashMap::new();
        for row in ctx
            .source
            .rows(&data, &[Arg::Text(&cursor), Arg::Text(&last)])
            .await?
        {
            data_rows
                .entry(text(&row, 0)?.unwrap_or_default())
                .or_default()
                .push(DataRow::read(&row, 1)?);
        }
        let mut chunk = Chunk::empty(EVENTS, number, &last);
        chunk.done = false;
        for row in &rows {
            chunk.progress.rows += 1;
            match event(
                ctx,
                targets,
                row,
                data_rows.get(&text(row, 0)?.unwrap_or_default()),
            )? {
                Some((record, provenance)) => {
                    chunk.records.push(record);
                    chunk.provenance.push(provenance);
                    chunk.progress.imported += 1;
                }
                None => chunk.progress.skipped += 1,
            }
        }
        commit(ctx, chunk).await?;
        cursor = last;
        step.chunks = number;
    }
}

/// One `website_event` row with its session as a record, or `None` when it is outside
/// retention, has no time, or belongs to nothing imported. Reconciliation counts each case.
fn event(
    ctx: &Ctx,
    targets: &Targets,
    row: &AnyRow,
    data: Option<&Vec<DataRow>>,
) -> Result<Option<(Record, Provenance)>, Failure> {
    let id = uuid(&text(row, 0)?.unwrap_or_default());
    let source = uuid(&text(row, 1)?.unwrap_or_default());
    let session = uuid(&text(row, 2)?.unwrap_or_default());
    let visit = text(row, 3)?.as_deref().map(uuid);
    let (Some(time), Some(event_type)) = (int(row, 4)?, int(row, 5)?) else {
        return Ok(None);
    };
    let Some(target) = targets.by_id.get(&source).copied() else {
        return Ok(None);
    };
    let kind = match (event_type, target) {
        (1, Target::Site) => EventKind::Pageview,
        (2, Target::Site) => EventKind::CustomEvent,
        (5, Target::Site) => EventKind::Performance,
        (3, Target::Link) => EventKind::LinkEvent,
        (4, Target::Pixel) => EventKind::PixelEvent,
        _ => return Ok(None),
    };
    let (policies, cutoff) = targets.policies(target);
    if time < cutoff {
        return Ok(None);
    }
    let local_day = policies.identity_day(time).local_day();
    let field = |i: usize| -> Result<Option<String>, Failure> {
        Ok(
            match (text(row, i)?, targets.widths.get(i).copied().flatten()) {
                (Some(value), Some(max)) => Some(capped(value, max)),
                (value, _) => value,
            },
        )
    };
    let dimensions = Dimensions {
        path: field(6)?,
        query: field(7)?.filter(|q| !q.is_empty()),
        referrer_path: field(8)?,
        referrer_domain: field(9)?,
        title: field(10)?,
        event_name: field(11)?,
        tag: field(12)?,
        hostname: field(13)?,
        utm_source: field(14)?,
        utm_medium: field(15)?,
        utm_campaign: field(16)?,
        utm_content: field(17)?,
        utm_term: field(18)?,
        click_id: field(19)?,
        browser: field(25)?,
        os: field(26)?,
        device: field(27)?,
        screen: field(28)?,
        language: field(29)?,
        country: field(30)?,
        region: field(31)?,
        city: field(32)?,
        latitude: None,
        longitude: None,
    };
    let performance = (kind == EventKind::Performance).then(|| {
        let ms = |i: usize| measure(field(i).ok().flatten(), 0);
        Performance {
            lcp: ms(20),
            fcp: ms(21),
            ttfb: ms(22),
            inp: ms(23),
            cls: measure(field(24).ok().flatten(), 4),
        }
    });
    let mut notes = Notes::default();
    let (properties, revenue) = match data.map(|d| data_object(d)) {
        Some(object) => {
            let raw = RawValue::from_string(object).map_err(super::internal)?;
            let revenue = (kind == EventKind::CustomEvent)
                .then(|| properties::revenue(&raw, &mut notes))
                .flatten();
            (properties::properties(&raw, &mut notes), revenue)
        }
        None => (Vec::new(), None),
    };
    let record = Record::Event(Event {
        id,
        site: source,
        kind,
        event_time_ms: time,
        receive_time_ms: time,
        local_day,
        daily_id: Some(ctx.daily_id(source, session, local_day)),
        profile_id: None,
        client_event_id: None,
        dimensions,
        performance,
        revenue,
        properties,
    });
    Ok(Some((
        record,
        Provenance {
            event: id,
            site: source,
            session,
            visit,
            distinct_id: field(33)?,
        },
    )))
}

/// The first `max` characters of `value`, without copying it.
fn capped(mut value: String, max: usize) -> String {
    if let Some((at, _)) = value.char_indices().nth(max) {
        value.truncate(at);
    }
    value
}

/// A web vital from its exact decimal text: milliseconds, or `cls` in units of 0.0001.
fn measure(text: Option<String>, scale: u32) -> Option<i32> {
    Decimal::parse(&text?)?
        .scaled(scale)
        .and_then(|v| i32::try_from(v).ok())
}

/// One `event_data` or `session_data` value.
struct DataRow {
    key: String,
    string: Option<String>,
    number: Option<String>,
    date_ms: Option<i64>,
    data_type: i64,
}

impl DataRow {
    fn read(row: &AnyRow, from: usize) -> Result<Self, Failure> {
        Ok(Self {
            key: text(row, from)?.unwrap_or_default(),
            string: text(row, from + 1)?,
            number: text(row, from + 2)?,
            date_ms: int(row, from + 3)?,
            data_type: int(row, from + 4)?.unwrap_or(1),
        })
    }

    /// The value as JSON text the collection property rules read: numbers keep their exact
    /// decimal text, dates become RFC 3339, arrays their JSON.
    fn json(&self) -> String {
        let string = || {
            self.string.as_deref().map_or("null".to_owned(), |s| {
                serde_json::Value::from(s).to_string()
            })
        };
        match self.data_type {
            2 => self
                .number
                .clone()
                .filter(|n| Decimal::parse(n).is_some())
                .unwrap_or_else(|| "null".into()),
            3 => match self.string.as_deref() {
                Some("true") => "true".into(),
                Some("false") => "false".into(),
                _ => string(),
            },
            4 => match self
                .date_ms
                .and_then(chrono::DateTime::from_timestamp_millis)
            {
                Some(date) => serde_json::Value::from(
                    date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                )
                .to_string(),
                None => string(),
            },
            5 => self
                .string
                .clone()
                .filter(|s| serde_json::from_str::<Vec<serde_json::Value>>(s).is_ok())
                .unwrap_or_else(string),
            _ => string(),
        }
    }
}

/// Flattened Umami data rows as one JSON object, keys as stored.
fn data_object(rows: &[DataRow]) -> String {
    let members: Vec<String> = rows
        .iter()
        .map(|r| format!("{}:{}", serde_json::Value::from(r.key.as_str()), r.json()))
        .collect();
    format!("{{{}}}", members.join(","))
}

/// `session_data` rows as identify records: each value attaches to the session of its
/// imported identity open at its time (0002 Identify).
async fn identify(ctx: &mut Ctx, targets: &Targets) -> Result<(), Failure> {
    let mut step = state::step(&ctx.db, IDENTIFY)
        .await
        .map_err(super::internal)?;
    if step.done {
        return Ok(());
    }
    let engine = ctx.source.engine;
    let select = format!(
        "SELECT {}, {}, {}, {}, {}, {}, {}, {}, {} FROM session_data
         WHERE session_data_id > {} ORDER BY session_data_id LIMIT ?",
        engine.text("session_data_id"),
        engine.text("website_id"),
        engine.text("session_id"),
        engine.text("data_key"),
        engine.text("string_value"),
        engine.text("number_value"),
        engine.ms("date_value"),
        engine.int("data_type"),
        engine.ms("created_at"),
        engine.id_param(),
    );
    let mut cursor = step.cursor.clone().unwrap_or_else(|| FIRST.into());
    loop {
        let rows = ctx
            .source
            .rows(
                &select,
                &[Arg::Text(&cursor), Arg::Int(ctx.options.chunk_rows)],
            )
            .await?;
        let number = step.chunks + 1;
        let Some(last) = rows.last() else {
            return commit(ctx, Chunk::empty(IDENTIFY, number, &cursor)).await;
        };
        let last = text(last, 0)?.unwrap_or_default();
        let mut chunk = Chunk::empty(IDENTIFY, number, &last);
        chunk.done = false;
        for row in &rows {
            chunk.progress.rows += 1;
            let site = uuid(&text(row, 1)?.unwrap_or_default());
            let session = uuid(&text(row, 2)?.unwrap_or_default());
            let time = int(row, 8)?;
            let (policies, cutoff) = targets.policies(Target::Site);
            let (Some(time), Some(Target::Site)) = (time, targets.by_id.get(&site).copied()) else {
                chunk.progress.skipped += 1;
                continue;
            };
            if time < cutoff {
                chunk.progress.skipped += 1;
                continue;
            }
            let value = DataRow::read(row, 3)?;
            let raw = RawValue::from_string(data_object(std::slice::from_ref(&value)))
                .map_err(super::internal)?;
            let local_day = policies.identity_day(time).local_day();
            chunk.records.push(Record::Identify(Identify {
                id: uuid(&text(row, 0)?.unwrap_or_default()),
                site,
                event_time_ms: time,
                receive_time_ms: time,
                local_day,
                daily_id: Some(ctx.daily_id(site, session, local_day)),
                profile_id: None,
                properties: properties::properties(&raw, &mut Notes::default()),
            }));
            chunk.progress.imported += 1;
        }
        commit(ctx, chunk).await?;
        cursor = last;
        step.chunks = number;
    }
}

/// Heatmap interactions of the last 90 UTC days (0011 Heatmaps), by recorder visit.
async fn heatmap_events(ctx: &mut Ctx, targets: &Targets) -> Result<(), Failure> {
    let mut step = state::step(&ctx.db, HEATMAPS)
        .await
        .map_err(super::internal)?;
    if step.done {
        return Ok(());
    }
    let engine = ctx.source.engine;
    let ints: Vec<String> = [
        "event_type",
        "page_x",
        "page_y",
        "viewport_w",
        "viewport_h",
        "scroll_pct",
    ]
    .iter()
    .map(|c| engine.int(c))
    .collect();
    let select = format!(
        "SELECT {}, {}, {}, {}, {}, {} FROM heatmap_event
         WHERE heatmap_event_id > {} ORDER BY heatmap_event_id LIMIT ?",
        engine.text("heatmap_event_id"),
        engine.text("website_id"),
        engine.text("visit_id"),
        engine.text("url_path"),
        ints.join(", "),
        engine.ms("created_at"),
        engine.id_param(),
    );
    let first_day = heatmaps::first_retained_day(ctx.clock_ms);
    let mut cursor = step.cursor.clone().unwrap_or_else(|| FIRST.into());
    loop {
        let rows = ctx
            .source
            .rows(
                &select,
                &[Arg::Text(&cursor), Arg::Int(ctx.options.chunk_rows)],
            )
            .await?;
        let number = step.chunks + 1;
        let Some(last) = rows.last() else {
            return commit(ctx, Chunk::empty(HEATMAPS, number, &cursor)).await;
        };
        let last = text(last, 0)?.unwrap_or_default();
        let mut chunk = Chunk::empty(HEATMAPS, number, &last);
        chunk.done = false;
        for row in &rows {
            chunk.progress.rows += 1;
            let site = uuid(&text(row, 1)?.unwrap_or_default());
            let v = |i: usize| int(row, i);
            let kind = match (v(4)?, v(5)?, v(6)?, v(9)?) {
                (Some(1), Some(x), Some(y), _) => Some(Interaction::Click {
                    x: x.clamp(0, i64::from(u16::MAX)) as u16,
                    page_y: y.clamp(0, i64::from(u32::MAX)) as u32,
                }),
                (Some(2), _, _, Some(percent)) => Some(Interaction::Scroll {
                    percent: percent.clamp(0, 100) as u8,
                }),
                _ => None,
            };
            let time = int(row, 10)?;
            let width = v(7)?.filter(|w| *w >= 1);
            let record = match (kind, time, width, targets.by_id.get(&site)) {
                (Some(kind), Some(time), Some(width), Some(Target::Site))
                    if heatmaps::utc_day(time) >= first_day =>
                {
                    Record::HeatmapInteraction(HeatmapInteraction {
                        id: uuid(&text(row, 0)?.unwrap_or_default()),
                        site,
                        capture_id: uuid(&text(row, 2)?.unwrap_or_default()),
                        event_time_ms: time,
                        receive_time_ms: time,
                        path: text(row, 3)?.unwrap_or_default(),
                        viewport_width: width.min(i64::from(u16::MAX)) as u16,
                        viewport_height: v(8)?.unwrap_or(0).clamp(0, i64::from(u16::MAX)) as u16,
                        kind,
                    })
                }
                _ => {
                    chunk.progress.skipped += 1;
                    continue;
                }
            };
            chunk.records.push(record);
            chunk.progress.imported += 1;
        }
        commit(ctx, chunk).await?;
        cursor = last;
        step.chunks = number;
    }
}

/// `session_link` rows tie Umami sessions to a distinct ID across visits. Holocron does not
/// infer identity continuity (0001), so they stay in the inventory as restricted provenance.
async fn session_links(ctx: &mut Ctx) -> Result<(), Failure> {
    let mut step = state::step(&ctx.db, SESSION_LINKS)
        .await
        .map_err(super::internal)?;
    if step.done {
        return Ok(());
    }
    const COLUMNS: [(&str, Kind); 4] = [
        ("website_id", Kind::Text),
        ("distinct_id", Kind::Text),
        ("session_id", Kind::Text),
        ("created_at", Kind::Ms),
    ];
    let engine = ctx.source.engine;
    let select: Vec<String> = COLUMNS
        .iter()
        .map(|(name, kind)| engine.column(name, *kind))
        .collect();
    // The primary key, (website_id, distinct_id, session_id), orders the chunks.
    let select = format!(
        "SELECT {} FROM session_link
         WHERE (website_id, distinct_id, session_id) > (?::uuid, ?, ?::uuid)
         ORDER BY website_id, distinct_id, session_id LIMIT ?",
        select.join(", ")
    );
    // The cursor is the JSON array of the last committed key.
    let mut cursor = step
        .cursor
        .clone()
        .unwrap_or_else(|| serde_json::json!([FIRST, "", FIRST]).to_string());
    loop {
        let last: [String; 3] = serde_json::from_str(&cursor).map_err(super::internal)?;
        let rows = ctx
            .source
            .rows(
                &select,
                &[
                    Arg::Text(&last[0]),
                    Arg::Text(&last[1]),
                    Arg::Text(&last[2]),
                    Arg::Int(ctx.options.chunk_rows),
                ],
            )
            .await?
            .iter()
            .map(|row| source::decode(row, &COLUMNS))
            .collect::<Result<Vec<_>, _>>()
            .map_err(super::source_error)?;
        let number = step.chunks + 1;
        let key = |row: &source::SourceRow| {
            serde_json::json!([
                row.text("website_id"),
                row.text("distinct_id"),
                row.text("session_id")
            ])
            .to_string()
        };
        let Some(last) = rows.last().map(key) else {
            return commit(ctx, Chunk::empty(SESSION_LINKS, number, &cursor)).await;
        };
        let mut chunk = Chunk::empty(SESSION_LINKS, number, &last);
        chunk.done = false;
        for row in rows {
            chunk.progress.rows += 1;
            chunk.progress.skipped += 1;
            chunk.inventory.push(Entry::new(
                "session_link",
                key(&row),
                "identity_link_not_imported",
                row.into(),
            ));
        }
        commit(ctx, chunk).await?;
        cursor = last;
        step.chunks = number;
    }
}

impl Chunk {
    /// A chunk with nothing to admit. Built at the end of a step it marks the step done.
    fn empty(step: &'static str, number: i64, cursor: &str) -> Self {
        Self {
            step,
            number,
            cursor: cursor.to_owned(),
            records: Vec::new(),
            provenance: Vec::new(),
            inventory: Vec::new(),
            progress: Progress::default(),
            done: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, data_type: i64, string: Option<&str>, number: Option<&str>) -> DataRow {
        DataRow {
            key: key.into(),
            string: string.map(Into::into),
            number: number.map(Into::into),
            date_ms: None,
            data_type,
        }
    }

    #[test]
    fn umami_data_rows_take_the_collection_types_and_revenue() {
        let rows = [
            row("plan", 1, Some("pro"), None),
            row("revenue", 2, None, Some("49.9000")),
            row("currency", 1, Some("EUR"), None),
            row("seats", 2, None, Some("3.0000")),
            row("trial", 3, Some("false"), None),
        ];
        let raw = RawValue::from_string(data_object(&rows)).unwrap();
        let mut notes = Notes::default();
        let props = properties::properties(&raw, &mut notes);
        use holocron_storage::PropertyValue::*;
        let values: Vec<_> = props
            .iter()
            .map(|p| (p.key.as_str(), p.value.clone()))
            .collect();
        assert_eq!(
            values,
            [
                ("plan", String("pro".into())),
                ("revenue", Decimal(499_000)),
                ("currency", String("EUR".into())),
                ("seats", Integer(3)),
                ("trial", Bool(false)),
            ]
        );
        let revenue = properties::revenue(&raw, &mut notes).unwrap();
        assert_eq!(
            (revenue.currency.as_str(), revenue.minor_units),
            ("EUR", 4990)
        );
    }

    #[test]
    fn web_vitals_keep_their_exact_units() {
        assert_eq!(measure(Some("1800.0".into()), 0), Some(1800));
        assert_eq!(measure(Some("0.0500".into()), 4), Some(500));
        assert_eq!(measure(None, 0), None);
    }
}
