//! The session list report (0004 Report definitions): sessions starting in range, with a
//! filter those with at least one matching event, paged in start order with ties by session
//! ID. Session fields come from the values correction stores on `sessions`; entry and exit
//! paths are read for the requested page only.

use std::collections::HashMap;

use holocron_protocol::exploration::{
    Cell, SessionColumn, SessionListReport, SessionListResult, SessionOrder,
};

use super::matching::{self, Reads};
use super::{Input, Outcome};
use crate::explore::sql::{self, Sql};

/// One session of the page, every column read.
struct Session {
    id: i64,
    person: bool,
    start: i64,
    last: i64,
    pageviews: i64,
    custom_events: i64,
    text: [Option<String>; 9],
}

/// The stored session fields of [`Session::text`], in order; `channel` is computed last.
const TEXT: [&str; 8] = [
    "referrer_domain",
    "utm_source",
    "utm_campaign",
    "country",
    "city",
    "device",
    "browser",
    "os",
];

pub fn run(input: &Input, report: &SessionListReport) -> Outcome<SessionListResult> {
    let (start, end) = input.read;
    let site = input.ctx.site;
    let mut tables = vec!["sl_s".to_owned()];
    if input.query.filter.is_some() {
        matching::materialize(
            input,
            "sl",
            Reads {
                sessions: true,
                ..Reads::default()
            },
            input.read,
        )?;
        tables.extend(["sl_ev".to_owned(), "sl_se".to_owned()]);
    }
    let mut entry = Sql::default();
    entry
        .push("CREATE OR REPLACE TEMP TABLE sl_s AS SELECT * FROM sessions WHERE site = ")
        .bind(site)
        .push(" AND start_time >= epoch_ms(")
        .bind(start)
        .push(") AND start_time < epoch_ms(")
        .bind(end)
        .push(")");
    if input.query.filter.is_some() {
        entry.push(" AND session_id IN (SELECT session_id FROM sl_se)");
    }
    matching::execute(input, &entry)?;
    let row_count: i64 =
        input
            .snapshot
            .connection()
            .query_row("SELECT count(*) FROM sl_s", [], |r| r.get(0))?;

    let direction = match report.order {
        SessionOrder::StartDesc => "DESC",
        SessionOrder::StartAsc => "ASC",
    };
    let mut page = Sql::default();
    page.push(&format!(
        "SELECT session_id, population = 2, epoch_ms(start_time), epoch_ms(last_time), pageviews,
                custom_events, {}, ",
        TEXT.join(", ")
    ));
    sql::channel(
        "lower(touch_query)",
        "lower(referrer_domain)",
        "lower(utm_medium)",
        "lower(utm_source)",
        input.ctx.hostnames,
        &mut page,
    );
    page.push(&format!(
        " FROM sl_s ORDER BY start_time {direction}, session_id LIMIT {} OFFSET {}",
        input.query.main.size, input.query.main.offset
    ));
    let sessions = matching::rows(input, &page, |r| {
        let mut text: [Option<String>; 9] = Default::default();
        for (i, slot) in text.iter_mut().enumerate() {
            *slot = r.get(6 + i)?;
        }
        Ok(Session {
            id: r.get(0)?,
            person: r.get(1)?,
            start: r.get(2)?,
            last: r.get(3)?,
            pageviews: r.get(4)?,
            custom_events: r.get(5)?,
            text,
        })
    })?;

    let wants_paths = report
        .columns
        .iter()
        .any(|c| matches!(c, SessionColumn::EntryPath | SessionColumn::ExitPath));
    let paths = if wants_paths && !sessions.is_empty() {
        paths(input, &sessions)?
    } else {
        HashMap::new()
    };
    matching::drop(input, &tables)?;

    let text = |value: &Option<String>| value.clone().map_or(Cell::NotSet, Cell::String);
    let rows = sessions
        .iter()
        .map(|s| {
            report
                .columns
                .iter()
                .map(|column| match column {
                    SessionColumn::SessionId => Cell::Integer(s.id),
                    SessionColumn::IdentityKind => {
                        Cell::String(if s.person { "person" } else { "daily" }.to_owned())
                    }
                    SessionColumn::Start => Cell::Integer(s.start),
                    SessionColumn::End => Cell::Integer(s.last),
                    SessionColumn::Span => Cell::Integer(s.last - s.start),
                    SessionColumn::Pageviews => Cell::Integer(s.pageviews),
                    SessionColumn::CustomEvents => Cell::Integer(s.custom_events),
                    SessionColumn::EntryPath => text(&paths.get(&s.id).and_then(|p| p.0.clone())),
                    SessionColumn::ExitPath => text(&paths.get(&s.id).and_then(|p| p.1.clone())),
                    SessionColumn::ReferrerDomain => text(&s.text[0]),
                    SessionColumn::UtmSource => text(&s.text[1]),
                    SessionColumn::UtmCampaign => text(&s.text[2]),
                    SessionColumn::Country => text(&s.text[3]),
                    SessionColumn::City => text(&s.text[4]),
                    SessionColumn::Device => text(&s.text[5]),
                    SessionColumn::Browser => text(&s.text[6]),
                    SessionColumn::Os => text(&s.text[7]),
                    SessionColumn::Channel => text(&s.text[8]),
                })
                .collect()
        })
        .collect();
    Ok(Ok(SessionListResult {
        rows,
        row_count,
        coverage: matching::coverage(input, false)?,
    }))
}

/// First and last pageview path of a session.
type Paths = (Option<String>, Option<String>);

/// First and last pageview path of each session on the page.
fn paths(
    input: &Input,
    sessions: &[Session],
) -> holocron_storage::duckdb::Result<HashMap<i64, Paths>> {
    let lo = sessions.iter().map(|s| s.start).min().unwrap_or_default();
    let hi = sessions.iter().map(|s| s.last).max().unwrap_or_default();
    let site = input.ctx.site;
    let mut sql = Sql::default();
    sql.push(
        "SELECT m.session_id, arg_min(e.path, (m.event_time, m.seq)), arg_max(e.path, (m.event_time, m.seq))
         FROM session_members m JOIN events e ON e.seq = m.seq AND e.site = ",
    )
    .bind(site)
    .push(" AND e.kind = 1 AND e.event_time >= epoch_ms(")
    .bind(lo)
    .push(") AND e.event_time <= epoch_ms(")
    .bind(hi)
    .push(") WHERE m.site = ")
    .bind(site)
    .push(" AND m.role = 1 AND m.event_time >= epoch_ms(")
    .bind(lo)
    .push(") AND m.event_time <= epoch_ms(")
    .bind(hi)
    .push(") AND m.session_id IN (");
    for (i, session) in sessions.iter().enumerate() {
        if i > 0 {
            sql.push(", ");
        }
        sql.bind(session.id);
    }
    sql.push(") GROUP BY m.session_id");
    Ok(matching::rows(input, &sql, |r| {
        Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?)))
    })?
    .into_iter()
    .collect())
}
