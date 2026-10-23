//! Bounded, idempotent correction of derived data from dirty identity days.
//!
//! One step takes the oldest dirty days (at most `Limits::days`), rebuilds
//! every derived row those days can affect, refreshes the day aggregates and
//! rollups of every day those rows touch and clears the processed dirty
//! rows, all in one transaction on the analytical
//! writer. A crash rolls the step back, so a day is either still dirty with
//! its previous rows or fully rebuilt. Rebuilding from immutable events makes
//! a rerun produce the same rows.
//!
//! What a dirty day affects:
//! - daily sessions of that day: a daily pseudonym lives one identity day, so
//!   they are rebuilt from that day's events alone;
//! - for each profile with a record on that day, a span of its history around
//!   those records: profile sessions cross midnight, so the span grows until no
//!   eligible event of the profile lies within one window before or after it.
//!
//! Why that is enough: a session boundary is decided by two consecutive eligible
//! events, the window at the earlier one and the revocations between them. A
//! record on day D only changes the decisions of pairs it is part of, so it
//! merges or splits only sessions with an event less than one window from it.
//! A gap of at least one window on both sides of a span is a boundary in any
//! derivation, so the sessions inside it can be rebuilt from its events alone.
//! The window used is the largest the site ever had, since a gap is judged by
//! the window at its earlier event. Every other input that moves a boundary
//! marks dirty a day with an event next to it: a revocation the days around its
//! instant, a policy change the days from one maximum window before its
//! instant, an erasure or retention run the days next to what it removed.
//! Performance and identify rows are attached against every session near the
//! span, not only the rebuilt ones, because an identify attaches to the next
//! session of its local day when none is open.
//!
//! Session rule (0002 Timeout rule): per identity, order eligible events by
//! `(event_time, seq)`; a gap of at least the window in effect at the earlier
//! event starts a new session. Daily sessions also end at an identity-day
//! change; profile sessions also end at a consent revocation (0001 Revocation).
//! A session carries the policy version in effect at its first event.

use holocron_storage::{Storage, duckdb};

use crate::METRIC_VERSION;
use crate::identity::{DEFAULT_WINDOW_MINUTES, MAX_TIME_MS, MIN_TIME_MS};

/// Member roles in `session_members`.
pub mod role {
    /// Pageview or custom event: starts, extends and counts in the session.
    pub const ELIGIBLE: u8 = 1;
    /// Performance row inside `[start, ends_at)` of a session of its identity.
    pub const PERFORMANCE: u8 = 2;
    /// Identify message carrying session properties (0002 Identify, performance, and recorder messages).
    pub const IDENTIFY: u8 = 3;
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Dirty identity days rebuilt per step, bounding how long one step holds the writer.
    pub days: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { days: 64 }
    }
}

/// What one step rebuilt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Step {
    pub days: u64,
    pub profiles: u64,
    pub sessions: u64,
    /// Dirty days left after the step.
    pub remaining: u64,
}

/// Runs one correction step. Returns a zero step when nothing is dirty.
pub async fn step(storage: &Storage, limits: Limits) -> Result<Step, holocron_storage::Error> {
    Ok(step_within(storage, limits.days.max(1)).await?.0)
}

/// Runs one step of at most `days` days. A step that runs out of engine memory rolls back
/// and runs again with half the days, down to one day. Returns the step and the day limit
/// that fit, so a run keeps the smaller limit instead of failing again.
async fn step_within(
    storage: &Storage,
    mut days: usize,
) -> Result<(Step, usize), holocron_storage::Error> {
    loop {
        match storage.write(move |tx| correct(tx, days)).await {
            Err(error) if out_of_memory(&error) => {
                if days == 1 {
                    eprintln!(
                        "holocron-analytics: correcting a single identity day ran out of engine memory"
                    );
                    return Err(error);
                }
                days = days.div_ceil(2);
                eprintln!(
                    "holocron-analytics: correction step ran out of engine memory, retrying with {days} days"
                );
            }
            result => return result.map(|step| (step, days)),
        }
    }
}

/// Whether DuckDB refused a statement for lack of memory. The transaction is rolled back.
fn out_of_memory(error: &holocron_storage::Error) -> bool {
    matches!(
        error,
        holocron_storage::Error::Engine(duckdb::Error::DuckDBFailure(_, Some(message)))
            if message.starts_with("Out of Memory Error")
    )
}

/// Runs steps until no dirty day is left; returns the summed work.
pub async fn run_until_clean(
    storage: &Storage,
    limits: Limits,
) -> Result<Step, holocron_storage::Error> {
    let mut total = Step::default();
    let mut days = limits.days.max(1);
    loop {
        let (step, fitted) = step_within(storage, days).await?;
        days = fitted;
        total.days += step.days;
        total.profiles += step.profiles;
        total.sessions += step.sessions;
        total.remaining = step.remaining;
        if step.remaining == 0 {
            return Ok(total);
        }
    }
}

/// Keeps derived data corrected: runs steps whenever the watermark moves or
/// `wake` is notified, until `stop` turns true. Code that marks days dirty
/// outside transfer (recalculations, policy changes) notifies `wake`. Errors
/// are logged and retried on the next change.
pub async fn follow(
    storage: &Storage,
    limits: Limits,
    wake: &tokio::sync::Notify,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let mut watermark = storage.watermark();
    while !*stop.borrow() {
        if let Err(error) = run_until_clean(storage, limits).await {
            eprintln!("holocron-analytics: correction failed, retrying: {error}");
        }
        tokio::select! {
            _ = watermark.changed() => {}
            _ = wake.notified() => {}
            _ = stop.changed() => {}
        }
    }
}

const DEFAULT_WINDOW_MS: i64 = DEFAULT_WINDOW_MINUTES as i64 * 60_000;

/// How far before a session an identify can attach to it: the identify must lie on the
/// session's local date, which in any two timezones spans less than three days.
const NEXT_SESSION_REACH_MS: i64 = 3 * 86_400_000;

/// The instants every span reaches, widened by its window and `reach`, within the range
/// event times can take.
fn span_bounds(tx: &duckdb::Transaction, reach: i64) -> duckdb::Result<(i64, i64)> {
    let (from, to): (Option<i64>, Option<i64>) = tx.query_row(
        &format!("SELECT min(lo - window_ms - {reach}), max(hi + window_ms + {reach}) FROM spans"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((
        from.unwrap_or(0).max(MIN_TIME_MS),
        to.unwrap_or(0).min(MAX_TIME_MS),
    ))
}

fn correct(tx: &duckdb::Transaction, days: usize) -> duckdb::Result<Step> {
    tx.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE work_days AS
         SELECT site, local_day, max(batch_id) AS through
         FROM dirty_days GROUP BY site, local_day
         ORDER BY min(batch_id), site, local_day
         LIMIT {days}"
    ))?;
    let days: i64 = tx.query_row("SELECT count(*) FROM work_days", [], |r| r.get(0))?;
    if days == 0 {
        tx.execute_batch("DROP TABLE work_days")?;
        return Ok(Step::default());
    }
    tx.execute_batch(&format!(
        r#"
        -- Profiles with a record on a work day, the instants of those records, and the
        -- largest window the site ever had.
        CREATE OR REPLACE TEMP TABLE work_profiles AS
        WITH touched AS (
            SELECT e.site, e.profile_id, epoch_ms(e.event_time) AS t FROM events e
            JOIN work_days w ON e.site = w.site AND e.local_day = w.local_day
            WHERE e.profile_id IS NOT NULL
            UNION ALL
            SELECT p.site, p.profile_id, epoch_ms(p.event_time) FROM session_properties p
            JOIN work_days w ON p.site = w.site AND p.local_day = w.local_day
            WHERE p.profile_id IS NOT NULL
        ), reach AS (
            SELECT site, max(window_ms) AS window_ms FROM session_policies GROUP BY site
        )
        SELECT t.site, t.profile_id, min(t.t) AS first_ms, max(t.t) AS last_ms,
               coalesce(any_value(r.window_ms), {DEFAULT_WINDOW_MS}) AS window_ms
        FROM touched t LEFT JOIN reach r ON r.site = t.site
        GROUP BY t.site, t.profile_id;

        -- Per profile, the eligible events from `lo` to `hi` are rebuilt.
        CREATE OR REPLACE TEMP TABLE spans AS
        SELECT site, profile_id, window_ms, first_ms AS lo, last_ms AS hi FROM work_profiles;
        "#
    ))?;
    // Widen each span until no eligible event of its profile lies within one window
    // outside it: then its first and last events are session boundaries, whatever
    // the stored sessions say. Sessions near a span are taken whole, events near it
    // with every event of their local date, so a chain of busy days takes one round
    // per day.
    loop {
        let (from, to) = span_bounds(tx, 0)?;
        tx.execute_batch(&format!(
            "UPDATE spans p SET lo = least(p.lo, w.lo), hi = greatest(p.hi, w.hi)
             FROM (SELECT p.site, p.profile_id,
                          min(epoch_ms(s.start_time)) AS lo, max(epoch_ms(s.last_time)) AS hi
                   FROM sessions s JOIN spans p ON s.site = p.site AND s.identity = p.profile_id
                   WHERE s.population = 2
                     AND s.last_time >= epoch_ms({from}::BIGINT) AND s.start_time <= epoch_ms({to}::BIGINT)
                     AND epoch_ms(s.last_time) >= p.lo - p.window_ms
                     AND epoch_ms(s.start_time) <= p.hi + p.window_ms
                   GROUP BY p.site, p.profile_id) w
             WHERE w.site = p.site AND w.profile_id = p.profile_id;"
        ))?;
        let (from, to) = span_bounds(tx, 0)?;
        let widened = tx.execute(
            &format!(
                "UPDATE spans p SET lo = least(p.lo, w.lo), hi = greatest(p.hi, w.hi)
                 FROM (SELECT x.site, x.profile_id,
                              min(epoch_ms(e.event_time)) AS lo, max(epoch_ms(e.event_time)) AS hi
                       FROM (SELECT DISTINCT e.site, e.profile_id, e.local_day
                             FROM events e JOIN spans p ON e.site = p.site AND e.profile_id = p.profile_id
                             WHERE e.event_time >= epoch_ms({from}::BIGINT) AND e.event_time <= epoch_ms({to}::BIGINT)
                               AND e.kind IN (1, 2)
                               AND (epoch_ms(e.event_time) >= p.lo - p.window_ms AND epoch_ms(e.event_time) < p.lo
                                    OR epoch_ms(e.event_time) > p.hi AND epoch_ms(e.event_time) <= p.hi + p.window_ms)) x
                       JOIN events e ON e.site = x.site AND e.profile_id = x.profile_id AND e.local_day = x.local_day
                       GROUP BY x.site, x.profile_id) w
                 WHERE w.site = p.site AND w.profile_id = p.profile_id"
            ),
            [],
        )?;
        if widened == 0 {
            break;
        }
    }
    // Literal bounds around every span, so the scans below skip row groups by time.
    let (from, to) = span_bounds(tx, NEXT_SESSION_REACH_MS)?;
    tx.execute_batch(&format!(
        r#"
        -- The profile sessions the spans replace. None reaches outside its span.
        CREATE OR REPLACE TEMP TABLE stale_profile AS
        SELECT s.site, s.session_id, s.local_day
        FROM sessions s JOIN spans p ON s.site = p.site AND s.identity = p.profile_id
        WHERE s.population = 2
          AND s.last_time >= epoch_ms({from}::BIGINT) AND s.start_time <= epoch_ms({to}::BIGINT)
          AND epoch_ms(s.last_time) >= p.lo - p.window_ms
          AND epoch_ms(s.start_time) <= p.hi + p.window_ms;
        "#
    ))?;
    tx.execute_batch(&format!(
        r#"
        -- Every event and identify whose session assignment the step rebuilds. Performance
        -- rows reach one window past the span, identifies one window and one local date.
        CREATE OR REPLACE TEMP TABLE scope AS
        SELECT e.seq, e.site, epoch_ms(e.event_time) AS t, e.kind, e.local_day,
               1::UTINYINT AS population, e.daily_id AS identity
        FROM events e JOIN work_days w ON e.site = w.site AND e.local_day = w.local_day
        WHERE e.profile_id IS NULL AND e.daily_id IS NOT NULL AND e.kind IN (1, 2, 3)
        UNION ALL
        SELECT e.seq, e.site, epoch_ms(e.event_time), e.kind, e.local_day, 2::UTINYINT, e.profile_id
        FROM events e JOIN spans p ON e.site = p.site AND e.profile_id = p.profile_id
        WHERE e.event_time >= epoch_ms({from}::BIGINT) AND e.event_time <= epoch_ms({to}::BIGINT)
          AND e.kind IN (1, 2, 3)
          AND epoch_ms(e.event_time) >= p.lo
          AND epoch_ms(e.event_time) <= p.hi + CASE WHEN e.kind = 3 THEN p.window_ms ELSE 0 END;

        CREATE OR REPLACE TEMP TABLE scope_identify AS
        SELECT DISTINCT sp.seq, sp.site, epoch_ms(sp.event_time) AS t, sp.local_day,
               1::UTINYINT AS population, sp.daily_id AS identity
        FROM session_properties sp JOIN work_days w ON sp.site = w.site AND sp.local_day = w.local_day
        WHERE sp.profile_id IS NULL AND sp.daily_id IS NOT NULL
        UNION ALL
        SELECT DISTINCT sp.seq, sp.site, epoch_ms(sp.event_time), sp.local_day, 2::UTINYINT, sp.profile_id
        FROM session_properties sp JOIN spans p ON sp.site = p.site AND sp.profile_id = p.profile_id
        WHERE sp.event_time >= epoch_ms({from}::BIGINT) AND sp.event_time <= epoch_ms({to}::BIGINT)
          AND epoch_ms(sp.event_time) >= p.lo - {NEXT_SESSION_REACH_MS}
          AND epoch_ms(sp.event_time) <= p.hi + p.window_ms;

        CREATE OR REPLACE TEMP TABLE stale AS
        SELECT s.site, s.session_id, s.local_day FROM sessions s
        JOIN work_days w ON s.site = w.site AND s.local_day = w.local_day
        WHERE s.population = 1
        UNION ALL
        SELECT site, session_id, local_day FROM stale_profile;

        CREATE OR REPLACE TEMP TABLE derived AS
        WITH eligible AS (
            SELECT s.seq, s.site, s.t, s.kind, s.local_day, s.population, s.identity,
                   coalesce(p.window_ms, {DEFAULT_WINDOW_MS}) AS window_ms,
                   coalesce(p.version, 1) AS version
            FROM scope s ASOF LEFT JOIN session_policies p ON s.site = p.site AND s.t >= p.effective_ms
            WHERE s.kind IN (1, 2)
        ), gaps AS (
            SELECT *, lag(t) OVER w AS prev_t, lag(window_ms) OVER w AS prev_window,
                   lag(local_day) OVER w AS prev_day
            FROM eligible
            WINDOW w AS (PARTITION BY site, population, identity ORDER BY t, seq)
        ), opened AS (
            SELECT *, (
                prev_t IS NULL
                OR t - prev_t >= prev_window
                OR (population = 1 AND local_day <> prev_day)
                OR (population = 2 AND EXISTS (
                    SELECT 1 FROM consent_revocations r
                    WHERE r.site = gaps.site
                      AND (r.profile_id IS NULL OR r.profile_id = gaps.identity)
                      AND r.revoked_at_ms > gaps.prev_t AND r.revoked_at_ms <= gaps.t))
            ) AS opens
            FROM gaps
        ), numbered AS (
            SELECT *, sum(opens::INTEGER) OVER (
                PARTITION BY site, population, identity ORDER BY t, seq ROWS UNBOUNDED PRECEDING) AS n
            FROM opened
        )
        SELECT seq, site, t, kind, local_day, population, identity, window_ms, version,
               first_value(seq) OVER (PARTITION BY site, population, identity, n ORDER BY t, seq) AS session_id
        FROM numbered;

        CREATE OR REPLACE TEMP TABLE rebuilt AS
        SELECT site, session_id, first(population) AS population, first(identity) AS identity,
               first(local_day ORDER BY t, seq) AS local_day,
               min(t) AS start_ms, max(t) AS last_ms,
               max(t) + last(window_ms ORDER BY t, seq) AS ends_ms,
               count(*) FILTER (WHERE kind = 1) AS pageviews,
               count(*) FILTER (WHERE kind = 2) AS custom_events,
               first(version ORDER BY t, seq) AS policy_version,
               arg_min(seq, (kind <> 1, t, seq)) AS touch_seq
        FROM derived GROUP BY site, session_id;

        -- Event times are whole milliseconds, so the first member by (t, seq) is the event
        -- whose sequence is the session ID. The touch is the first pageview, else that event.
        CREATE OR REPLACE TEMP TABLE new_sessions AS
        SELECT r.site, r.session_id, r.population, r.identity, r.local_day,
               epoch_ms(r.start_ms) AS start_time, epoch_ms(r.last_ms) AS last_time,
               epoch_ms(r.ends_ms) AS ends_at, r.pageviews, r.custom_events, r.policy_version,
               {session_fields}
        FROM rebuilt r
        JOIN events f ON f.site = r.site AND f.seq = r.session_id
        JOIN events t ON t.site = r.site AND t.seq = r.touch_seq;

        -- Sessions a performance or identify row in scope can attach to: the rebuilt ones and
        -- the profile's unchanged sessions around its span.
        CREATE OR REPLACE TEMP TABLE neighbours AS
        SELECT site, session_id, population, identity, local_day, start_ms, ends_ms FROM rebuilt
        UNION ALL
        SELECT s.site, s.session_id, s.population, s.identity, s.local_day,
               epoch_ms(s.start_time), epoch_ms(s.ends_at)
        FROM sessions s JOIN spans p ON s.site = p.site AND s.identity = p.profile_id
        ANTI JOIN stale_profile x ON x.site = s.site AND x.session_id = s.session_id
        WHERE s.population = 2
          AND s.last_time >= epoch_ms({from}::BIGINT) AND s.start_time <= epoch_ms({to}::BIGINT)
          AND epoch_ms(s.last_time) >= p.lo - {NEXT_SESSION_REACH_MS} - p.window_ms
          AND epoch_ms(s.start_time) <= p.hi + p.window_ms + {NEXT_SESSION_REACH_MS};

        CREATE OR REPLACE TEMP TABLE new_members AS
        SELECT site, seq, session_id, {eligible}::UTINYINT AS role, epoch_ms(t) AS event_time FROM derived
        UNION ALL
        SELECT p.site, p.seq, s.session_id, {performance}, epoch_ms(p.t)
        FROM (SELECT * FROM scope WHERE kind = 3) p
        ASOF JOIN neighbours s
          ON p.site = s.site AND p.population = s.population AND p.identity = s.identity AND p.t >= s.start_ms
        WHERE p.t < s.ends_ms
        UNION ALL
        -- Identify: the session open at its time, else the next session of that identity on that day.
        (WITH open AS (
            SELECT i.seq, i.site, i.t, CASE WHEN i.t < s.ends_ms THEN s.session_id END AS session_id
            FROM scope_identify i ASOF LEFT JOIN neighbours s
              ON i.site = s.site AND i.population = s.population AND i.identity = s.identity AND i.t >= s.start_ms
        ), following AS (
            SELECT i.seq, i.site, CASE WHEN s.local_day = i.local_day THEN s.session_id END AS session_id
            FROM scope_identify i ASOF LEFT JOIN neighbours s
              ON i.site = s.site AND i.population = s.population AND i.identity = s.identity AND i.t < s.start_ms
        )
        SELECT o.site, o.seq, coalesce(o.session_id, f.session_id), {identify}, epoch_ms(o.t)
        FROM open o JOIN following f ON o.site = f.site AND o.seq = f.seq
        WHERE coalesce(o.session_id, f.session_id) IS NOT NULL);

        -- Only rows that changed are replaced, appended in (site, time) order: a profile's
        -- earlier sessions stay where they are, so both tables keep their time order. Sequences
        -- are unique across sites, and a member's event time follows from its sequence. Old rows
        -- are the members of stale sessions and the current attachments of the rows in scope,
        -- which may belong to an unchanged session.
        CREATE OR REPLACE TEMP TABLE old_members AS
        SELECT m.seq, m.session_id, m.role FROM session_members m
        SEMI JOIN stale x ON m.site = x.site AND m.session_id = x.session_id
        UNION
        SELECT m.seq, m.session_id, m.role FROM session_members m
        SEMI JOIN (SELECT site, seq FROM scope WHERE kind = 3 UNION ALL SELECT site, seq FROM scope_identify) c
          ON m.site = c.site AND m.seq = c.seq
        WHERE m.role <> {eligible};

        CREATE OR REPLACE TEMP TABLE changed_sessions AS
        SELECT s.* FROM sessions s SEMI JOIN stale x ON s.site = x.site AND s.session_id = x.session_id
        EXCEPT SELECT * FROM new_sessions;
        DELETE FROM sessions s USING changed_sessions x
        WHERE s.site = x.site AND s.session_id = x.session_id;
        INSERT INTO sessions
        SELECT * FROM new_sessions n
        ANTI JOIN sessions s ON s.site = n.site AND s.session_id = n.session_id
        ORDER BY n.site, n.start_time, n.session_id;

        DELETE FROM session_members m USING (
            SELECT * FROM old_members o
            ANTI JOIN new_members n ON n.seq = o.seq AND n.role = o.role AND n.session_id = o.session_id
        ) x
        WHERE m.seq = x.seq AND m.role = x.role;
        INSERT INTO session_members
        SELECT n.* FROM new_members n
        ANTI JOIN old_members o ON o.seq = n.seq AND o.role = n.role AND o.session_id = n.session_id
        ORDER BY n.site, n.event_time, n.seq;

        CREATE OR REPLACE TEMP TABLE refresh AS
        SELECT site, local_day FROM work_days
        UNION SELECT site, local_day FROM stale
        UNION SELECT site, local_day FROM rebuilt;

        DELETE FROM day_aggregates a USING refresh r WHERE a.site = r.site AND a.local_day = r.local_day;

        INSERT INTO day_aggregates
        WITH ev AS (
            SELECT e.site, e.local_day,
                   count(*) FILTER (WHERE e.kind = 1) AS pageviews,
                   count(*) FILTER (WHERE e.kind = 2) AS custom_events,
                   count(*) FILTER (WHERE e.kind IN (1, 2)) AS eligible_events,
                   count(*) FILTER (WHERE e.kind IN (1, 2) AND (e.daily_id IS NOT NULL OR e.profile_id IS NOT NULL)) AS identified_events,
                   count(*) FILTER (WHERE e.kind IN (1, 2) AND e.profile_id IS NOT NULL) AS linked_events,
                   count(DISTINCT e.daily_id) FILTER (WHERE e.kind IN (1, 2)) AS visitors
            FROM events e JOIN refresh r ON e.site = r.site AND e.local_day = r.local_day
            GROUP BY e.site, e.local_day
        ), se AS (
            SELECT s.site, s.local_day,
                   count(*) AS sessions,
                   count(*) FILTER (WHERE s.pageviews > 0) AS pageview_sessions,
                   count(*) FILTER (WHERE s.pageviews = 1 AND s.custom_events = 0) AS bounces,
                   coalesce(sum(epoch_ms(s.last_time) - epoch_ms(s.start_time)) FILTER (WHERE s.pageviews > 0), 0) AS span_sum_ms,
                   list(DISTINCT s.policy_version ORDER BY s.policy_version) AS versions
            FROM sessions s JOIN refresh r ON s.site = r.site AND s.local_day = r.local_day
            GROUP BY s.site, s.local_day
        )
        SELECT r.site, r.local_day, {METRIC_VERSION},
               coalesce(se.versions, []::INTEGER[]),
               coalesce(ev.pageviews, 0), coalesce(ev.custom_events, 0), coalesce(ev.eligible_events, 0),
               coalesce(ev.identified_events, 0), coalesce(ev.linked_events, 0), coalesce(ev.visitors, 0),
               coalesce(se.sessions, 0), coalesce(se.pageview_sessions, 0), coalesce(se.bounces, 0),
               coalesce(se.span_sum_ms, 0),
               (SELECT coalesce(max(batch_id), 0) FROM applied_batches)
        FROM refresh r
        LEFT JOIN ev ON ev.site = r.site AND ev.local_day = r.local_day
        LEFT JOIN se ON se.site = r.site AND se.local_day = r.local_day
        WHERE ev.site IS NOT NULL OR se.site IS NOT NULL;

        -- Rollups change on work days and on every day of a session, old or new, with an event
        -- on a work day: a profile session crossing midnight gives the next day's events their
        -- session fields. Any other session keeps its members, ID and first event.
        CREATE OR REPLACE TEMP TABLE rollup_days AS
        WITH assigned AS (
            SELECT d.site, d.local_day, d.session_id AS new_id, o.session_id AS old_id
            FROM derived d LEFT JOIN old_members o ON o.seq = d.seq AND o.role = {eligible}
        ), touching AS (
            SELECT a.* FROM assigned a JOIN work_days w ON a.site = w.site AND a.local_day = w.local_day
        )
        SELECT site, local_day FROM work_days
        UNION SELECT a.site, a.local_day FROM assigned a
              SEMI JOIN touching t ON a.site = t.site AND a.new_id = t.new_id
        UNION SELECT a.site, a.local_day FROM assigned a
              SEMI JOIN touching t ON a.site = t.site AND a.old_id = t.old_id;

        {rollups}

        DELETE FROM dirty_days d USING work_days w
        WHERE d.site = w.site AND d.local_day = w.local_day AND d.batch_id <= w.through;
        "#,
        rollups = crate::explore::rollup::refresh_sql(),
        session_fields = crate::explore::session_fields_sql(),
        eligible = role::ELIGIBLE,
        performance = role::PERFORMANCE,
        identify = role::IDENTIFY,
    ))?;
    let step = tx.query_row(
        "SELECT (SELECT count(*) FROM work_profiles), (SELECT count(*) FROM rebuilt),
                (SELECT count(*) FROM (SELECT DISTINCT site, local_day FROM dirty_days))",
        [],
        |r| {
            Ok(Step {
                days: days as u64,
                profiles: r.get::<_, i64>(0)? as u64,
                sessions: r.get::<_, i64>(1)? as u64,
                remaining: r.get::<_, i64>(2)? as u64,
            })
        },
    )?;
    tx.execute_batch(
        "DROP TABLE work_days; DROP TABLE work_profiles; DROP TABLE scope; DROP TABLE scope_identify;
         DROP TABLE stale; DROP TABLE derived; DROP TABLE rebuilt; DROP TABLE refresh;
         DROP TABLE old_members; DROP TABLE changed_sessions; DROP TABLE new_sessions; DROP TABLE new_members;
         DROP TABLE rollup_days; DROP TABLE stale_profile; DROP TABLE spans; DROP TABLE neighbours;",
    )?;
    Ok(step)
}
