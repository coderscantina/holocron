//! One session in detail: its stored fields, its member events in order and the latest value of
//! each session property identified into it. Membership is what correction stored in
//! `session_members`, so the detail agrees with the session list and every session measure.
//! The timeline reads the latest sessions of a window the same way, for drawing them over time.

use std::collections::HashMap;

use holocron_protocol::exploration::Cell;
use holocron_protocol::limits::{SESSION_TIMELINE_EVENTS_MAX, SESSION_TIMELINE_SESSIONS_MAX};
use holocron_storage::duckdb::{self, params_from_iter};
use holocron_storage::{Id, Snapshot};

use super::run::{IDENTIFY_LEAD_MS, decode_cell};
use super::sql::{Sql, channel};

/// Member events returned at most; a longer session says it was cut.
pub const EVENTS: usize = 500;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionDetail {
    pub id: i64,
    pub person: bool,
    /// Unix milliseconds of the first and last member event.
    pub start: i64,
    pub last: i64,
    pub pageviews: i64,
    pub custom_events: i64,
    /// Stored session fields by name, then `channel`, in [`FIELDS`] order.
    pub fields: Vec<(&'static str, Option<String>)>,
    pub events: Vec<SessionEvent>,
    /// More than [`EVENTS`] member events exist.
    pub truncated: bool,
    /// Latest value per key, by key.
    pub properties: Vec<(String, Cell)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    pub at: i64,
    pub pageview: bool,
    pub path: Option<String>,
    pub title: Option<String>,
    pub event_name: Option<String>,
}

/// The stored session fields the detail shows, in order; `channel` is computed after them.
pub const FIELDS: [&str; 13] = [
    "referrer_domain",
    "referrer_path",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "country",
    "region",
    "city",
    "device",
    "browser",
    "os",
    "language",
    "screen",
];

/// The session `id` of `site`, or `None` when the site has no such session.
pub fn session(
    snapshot: &Snapshot,
    site: Id,
    id: i64,
    hostnames: &[String],
) -> duckdb::Result<Option<SessionDetail>> {
    let conn = snapshot.connection();
    let mut head = Sql::default();
    head.push(&format!(
        "SELECT population = 2, epoch_ms(start_time), epoch_ms(last_time), epoch_ms(ends_at), pageviews,
                custom_events, {}, ",
        FIELDS.join(", ")
    ));
    channel(
        "lower(touch_query)",
        "lower(referrer_domain)",
        "lower(utm_medium)",
        "lower(utm_source)",
        hostnames,
        &mut head,
    );
    head.push(" FROM sessions WHERE site = ")
        .bind(site)
        .push(" AND session_id = ")
        .bind(id);
    let mut statement = conn.prepare(&head.text)?;
    let mut rows = statement.query(params_from_iter(head.params.iter()))?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let (person, start, last, ends): (bool, i64, i64, i64) =
        (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
    let (pageviews, custom_events): (i64, i64) = (row.get(4)?, row.get(5)?);
    let mut fields = Vec::with_capacity(FIELDS.len() + 1);
    for (i, name) in FIELDS.iter().enumerate() {
        fields.push((*name, row.get(6 + i)?));
    }
    fields.push(("channel", row.get(6 + FIELDS.len())?));

    // Members lie inside the session's span; the time bounds prune storage.
    let mut members = Sql::default();
    members
        .push(
            "SELECT epoch_ms(e.event_time), e.kind, e.path, e.title, e.event_name
             FROM session_members m JOIN events e ON e.seq = m.seq AND e.site = ",
        )
        .bind(site)
        .push(" AND e.event_time >= epoch_ms(")
        .bind(start)
        .push(") AND e.event_time <= epoch_ms(")
        .bind(last)
        .push(") WHERE m.site = ")
        .bind(site)
        .push(" AND m.role = 1 AND m.session_id = ")
        .bind(id)
        .push(" AND m.event_time >= epoch_ms(")
        .bind(start)
        .push(") AND m.event_time <= epoch_ms(")
        .bind(last)
        .push(&format!(
            ") ORDER BY m.event_time, m.seq LIMIT {}",
            EVENTS + 1
        ));
    let mut statement = conn.prepare(&members.text)?;
    let mut events = statement
        .query_map(params_from_iter(members.params.iter()), |r| {
            Ok(SessionEvent {
                at: r.get(0)?,
                pageview: r.get::<_, u8>(1)? == 1,
                path: r.get(2)?,
                title: r.get(3)?,
                event_name: r.get(4)?,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;
    let truncated = events.len() > EVENTS;
    events.truncate(EVENTS);

    // An identify attaches up to a local day before the start and until the session ends.
    let latest = |column: &str| format!("arg_max_null({column}, (p.event_time, p.seq))");
    let mut identified = Sql::default();
    identified
        .push(&format!(
            "SELECT p.key, {}, {}, {}, {}, {}, epoch_ms({}), {}
             FROM session_members m JOIN session_properties p ON p.seq = m.seq AND p.site = ",
            latest("p.type"),
            latest("p.v_string"),
            latest("p.v_integer"),
            latest("p.v_decimal"),
            latest("p.v_bool"),
            latest("p.v_date"),
            latest("p.v_array"),
        ))
        .bind(site)
        .push(" WHERE m.site = ")
        .bind(site)
        .push(" AND m.role = 3 AND m.session_id = ")
        .bind(id)
        .push(" AND m.event_time >= epoch_ms(")
        .bind(start - IDENTIFY_LEAD_MS)
        .push(") AND m.event_time <= epoch_ms(")
        .bind(ends)
        .push(") GROUP BY p.key ORDER BY p.key");
    let mut statement = conn.prepare(&identified.text)?;
    let properties = statement
        .query_map(params_from_iter(identified.params.iter()), |r| {
            let raw = (1..8)
                .map(|i| r.get::<_, duckdb::types::Value>(i))
                .collect::<duckdb::Result<Vec<_>>>()?;
            Ok((r.get::<_, String>(0)?, decode_cell(&raw)))
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;

    Ok(Some(SessionDetail {
        id,
        person,
        start,
        last,
        pageviews,
        custom_events,
        fields,
        events,
        truncated,
        properties,
    }))
}

/// The sessions starting in a window with their member events, for drawing them over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    /// Sessions starting in the window, also those past [`SESSION_TIMELINE_SESSIONS_MAX`].
    pub total: i64,
    /// The median observed span of those sessions with a pageview, in milliseconds, as the
    /// `span_median` measure reads it; `None` without one.
    pub median_span: Option<i64>,
    /// The latest [`SESSION_TIMELINE_SESSIONS_MAX`] by start, latest first.
    pub sessions: Vec<TimelineSession>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSession {
    pub id: i64,
    pub person: bool,
    /// Unix milliseconds of the first and last member event.
    pub start: i64,
    pub last: i64,
    pub pageviews: i64,
    pub custom_events: i64,
    pub country: Option<String>,
    pub device: Option<String>,
    pub browser: Option<String>,
    /// The first [`SESSION_TIMELINE_EVENTS_MAX`] member events by event time, without titles.
    pub events: Vec<SessionEvent>,
    /// More than [`SESSION_TIMELINE_EVENTS_MAX`] member events exist.
    pub truncated: bool,
}

/// The sessions of `site` starting in `[from, to)`, latest first, with their member events.
/// The same start rule as the session list report, so `total` agrees with its count.
pub fn timeline(snapshot: &Snapshot, site: Id, from: i64, to: i64) -> duckdb::Result<Timeline> {
    let conn = snapshot.connection();
    let latest = |sql: &mut Sql| {
        sql.push(" FROM sessions WHERE site = ")
            .bind(site)
            .push(" AND start_time >= epoch_ms(")
            .bind(from)
            .push(") AND start_time < epoch_ms(")
            .bind(to)
            .push(&format!(
                ") ORDER BY start_time DESC, session_id DESC LIMIT {SESSION_TIMELINE_SESSIONS_MAX}"
            ));
    };
    let mut head = Sql::default();
    head.push(
        "SELECT session_id, population = 2, epoch_ms(start_time), epoch_ms(last_time), pageviews,
                custom_events, country, device, browser, count(*) OVER (),
                quantile_disc(CASE WHEN pageviews > 0 THEN epoch_ms(last_time) - epoch_ms(start_time) END, 0.5) OVER ()",
    );
    latest(&mut head);
    let mut statement = conn.prepare(&head.text)?;
    let (mut total, mut median_span) = (0, None);
    let mut sessions = statement
        .query_map(params_from_iter(head.params.iter()), |r| {
            (total, median_span) = (r.get(9)?, r.get(10)?);
            Ok(TimelineSession {
                id: r.get(0)?,
                person: r.get(1)?,
                start: r.get(2)?,
                last: r.get(3)?,
                pageviews: r.get(4)?,
                custom_events: r.get(5)?,
                country: r.get(6)?,
                device: r.get(7)?,
                browser: r.get(8)?,
                events: Vec::new(),
                truncated: false,
            })
        })?
        .collect::<duckdb::Result<Vec<_>>>()?;
    let (Some(first), Some(last)) = (
        sessions.iter().map(|s| s.start).min(),
        sessions.iter().map(|s| s.last).max(),
    ) else {
        return Ok(Timeline {
            total,
            median_span,
            sessions,
        });
    };

    // Members lie inside their sessions' spans; the time bounds prune storage.
    let mut members = Sql::default();
    members.push("WITH s AS (SELECT session_id");
    latest(&mut members);
    members
        .push(
            ") SELECT m.session_id, epoch_ms(e.event_time), e.kind, e.path, e.event_name
             FROM session_members m SEMI JOIN s ON s.session_id = m.session_id
             JOIN events e ON e.seq = m.seq AND e.site = ",
        )
        .bind(site)
        .push(" AND e.event_time >= epoch_ms(")
        .bind(first)
        .push(") AND e.event_time <= epoch_ms(")
        .bind(last)
        .push(") WHERE m.site = ")
        .bind(site)
        .push(" AND m.role = 1 AND m.event_time >= epoch_ms(")
        .bind(first)
        .push(") AND m.event_time <= epoch_ms(")
        .bind(last)
        .push(&format!(
            ") QUALIFY row_number() OVER (PARTITION BY m.session_id ORDER BY m.event_time, m.seq) <= {}
             ORDER BY m.session_id, m.event_time, m.seq",
            SESSION_TIMELINE_EVENTS_MAX + 1
        ));
    let index: HashMap<i64, usize> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    let mut statement = conn.prepare(&members.text)?;
    let mut rows = statement.query(params_from_iter(members.params.iter()))?;
    while let Some(row) = rows.next()? {
        let Some(session) = index.get(&row.get::<_, i64>(0)?).map(|&i| &mut sessions[i]) else {
            continue;
        };
        if session.events.len() == SESSION_TIMELINE_EVENTS_MAX {
            session.truncated = true;
            continue;
        }
        session.events.push(SessionEvent {
            at: row.get(1)?,
            pageview: row.get::<_, u8>(2)? == 1,
            path: row.get(3)?,
            title: None,
            event_name: row.get(4)?,
        });
    }
    Ok(Timeline {
        total,
        median_span,
        sessions,
    })
}
