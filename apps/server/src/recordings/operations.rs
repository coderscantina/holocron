//! `recordings.*` and `heatmaps.*` operations. Every read and write resolves a SiteScope
//! first: a hidden site and a capture of another site both answer `not_found`. A share viewer
//! with the recordings grant sees only replays inside the share ([`ShareWindow`]).

use std::collections::HashSet;

use holocron_analytics::explore::{self, Context, Pred};
use holocron_analytics::identity::{MAX_TIME_MS, MIN_TIME_MS};
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::exploration::{
    Condition, DimensionField, Field, Group, GroupOp, Node, Operator, Value,
};
use holocron_protocol::limits::{PAGE_SIZE_DEFAULT, PAGE_SIZE_MAX};
use holocron_protocol::operations::recordings::{
    CaptureArgs, CaptureId, CaptureResult, CaptureSummary, HeatmapCells, HeatmapCellsArgs,
    HeatmapPages, HeatmapPagesArgs, RecordingBudget, RecordingList, RecordingPolicyResult,
    RecordingPolicyUpdateArgs, RecordingSiteArgs, RecordingsGetArgs, RecordingsHandlers,
    RecordingsListArgs, ReplayPage,
};
use holocron_protocol::operations::sharing::ShareTarget;
use holocron_protocol::registry::Snapshot;
use holocron_storage::duckdb::{Connection, OptionalExt};
use holocron_storage::{Lane, duckdb};
use serde_json::json;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::replay::{Assembler, PLAYBACK_STACK_BYTES};
use super::store::{Capture, Filter, Position, Saved, Store, id_of};
use super::{heatmaps, policy, replay_cutoff_ms};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::explorations::{hostnames, refused};
use crate::sharing::ShareViewer;
use crate::sites::policy::{analytics_id, session_policies};
use crate::sites::scope::SiteScope;
use crate::state::AppState;

/// Event JSON one playback page aims for, and the most decoded input it may read to finish a
/// split event.
const PAGE_BYTES: usize = 4 * 1024 * 1024;
const PAGE_HARD_BYTES: i64 = 16 * 1024 * 1024;
const PARTS_PER_READ: i64 = 8;
/// A replay starts shortly after the pageview that gave the recorder its token.
const SESSION_SLACK_MS: i64 = 60_000;
const POPULATION_DAILY: u8 = 1;
/// Batches of [`PAGE_SIZE_MAX`] replays one share viewer's list page reads at most.
const SHARED_SCANS: usize = 10;
const DAY_MS: i64 = 86_400_000;

/// What a share viewer sees of a site's replays (decision 0007 Share recordings grant): those
/// received within the share's range limit before now that match its pinned filter. Replays
/// outside answer like missing ones.
struct ShareWindow {
    since: i64,
    filter: Option<Group>,
}

impl ShareWindow {
    fn of(share: &ShareViewer, now: i64) -> Self {
        match &share.target {
            ShareTarget::Exploration { limits, .. } => Self {
                since: now - i64::from(limits.range_max_days) * DAY_MS,
                filter: limits.pinned_filter.clone(),
            },
            // `sharing.create` refuses recordings for board shares, which have no range limit
            // or pinned filter to keep to; a board share with the grant sees nothing.
            ShareTarget::Board { .. } => Self {
                since: i64::MAX,
                filter: None,
            },
        }
    }

    fn admits(&self, row: &Capture) -> bool {
        row.received_at >= self.since
            && self
                .filter
                .as_ref()
                .is_none_or(|group| group_matches(group, row))
    }
}

/// Whether a replay matches a filter group. A replay carries one device, browser, OS and
/// country; any other field cannot be decided per replay (a visit spans many pages and
/// events), so its condition does not match and the replay stays hidden. Groups have no
/// negation, so an undecided condition can only hide a replay, never reveal one.
fn group_matches(group: &Group, row: &Capture) -> bool {
    let matches = |node: &Node| match node {
        Node::Group(inner) => group_matches(inner, row),
        Node::Cond(condition) => condition_matches(condition, row),
    };
    match group.op {
        GroupOp::And => group.items.iter().all(matches),
        GroupOp::Or => group.items.iter().any(matches),
    }
}

fn text(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

fn condition_matches(condition: &Condition, row: &Capture) -> bool {
    let value = match condition.field {
        Field::Dimension(DimensionField::Device) => &row.device,
        Field::Dimension(DimensionField::Browser) => &row.browser,
        Field::Dimension(DimensionField::Os) => &row.os,
        Field::Dimension(DimensionField::Country) => &row.country,
        _ => return false,
    };
    let listed = |actual: &str| match &condition.value {
        Some(Value::List(items)) => items.iter().any(|item| text(item) == Some(actual)),
        _ => false,
    };
    let compared = condition.value.as_ref().and_then(text);
    match (condition.cmp, value.as_deref(), compared) {
        (Operator::IsSet, actual, _) => actual.is_some(),
        (Operator::IsNotSet, actual, _) => actual.is_none(),
        (Operator::Eq, Some(actual), Some(expected)) => actual == expected,
        (Operator::Neq, Some(actual), Some(expected)) => actual != expected,
        (Operator::Contains, Some(actual), Some(part)) => actual.contains(part),
        (Operator::NotContains, Some(actual), Some(part)) => !actual.contains(part),
        (Operator::StartsWith, Some(actual), Some(start)) => actual.starts_with(start),
        (Operator::In, Some(actual), _) => listed(actual),
        (Operator::NotIn, Some(actual), _) => !listed(actual),
        _ => false,
    }
}

impl RecordingsHandlers for AppState {
    async fn recordings_policy(
        &self,
        ctx: &mut QueryCtx,
        args: RecordingSiteArgs,
    ) -> Result<Snapshot<RecordingPolicyResult>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        let policy = policy::load(ctx.conn(), scope.site().as_str()).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: RecordingPolicyResult { policy },
        })
    }

    async fn recordings_update_policy(
        &self,
        ctx: &mut CommandCtx,
        args: RecordingPolicyUpdateArgs,
    ) -> Result<RecordingPolicyResult, ApiError> {
        let scope = ctx
            .site_scope(
                &args.site,
                Capability::SiteConfigure,
                args.expected_revision,
            )
            .await?;
        let site = scope.site().as_str();
        let current = policy::load(ctx.conn(), site).await?;
        let mut next = policy::validate(&args, current.version)?;
        if next == current {
            return Ok(RecordingPolicyResult { policy: current });
        }
        next.version = current.version + 1;
        let now = ctx.now();
        policy::store(ctx.conn(), site, &next, now).await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.recording_policy_changed",
            json!(next),
        )
        .await?;
        Ok(RecordingPolicyResult { policy: next })
    }

    async fn recordings_budget(
        &self,
        ctx: &mut QueryCtx,
        args: RecordingSiteArgs,
    ) -> Result<Snapshot<RecordingBudget>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        let r = &self.recordings;
        let usage = r.store.usage().await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: RecordingBudget {
                used_bytes: usage.bytes,
                cap_bytes: r.cap_bytes() as i64,
                saved_bytes: usage.saved_bytes,
                saved_cap_bytes: r.saved_cap_bytes() as i64,
                captures: usage.captures,
                file_bytes: usage.file_bytes,
                refusing: r.refusing(self.collector.clock().now_ms()),
            },
        })
    }

    async fn recordings_list(
        &self,
        ctx: &mut QueryCtx,
        args: RecordingsListArgs,
    ) -> Result<Snapshot<RecordingList>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead)
            .await?;
        let mut errors = FieldErrors::default();
        range(args.start, args.end, &mut errors);
        let limit = args.limit.unwrap_or(PAGE_SIZE_DEFAULT);
        if !(1..=PAGE_SIZE_MAX).contains(&limit) {
            errors.add("limit", FieldErrorCode::InvalidValue, None);
        }
        let before = match args.after.as_deref().map(cursor) {
            Some(None) => {
                errors.add("after", FieldErrorCode::InvalidFormat, None);
                None
            }
            other => other.flatten(),
        };
        errors.finish()?;
        let matching = match args.filter.as_deref() {
            Some(text) => {
                let group: Group = serde_json::from_str(text).map_err(|error| {
                    ApiError::with_message(ErrorCode::MalformedRequest, format!("filter: {error}"))
                })?;
                Some(explore::validate_filter(&group).map_err(refused)?)
            }
            None => None,
        };

        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let session = match args.session {
            Some(session) => {
                let found = self
                    .collector
                    .storage()
                    .read(Lane::Report, move |s| {
                        session_window(s.connection(), site, session)
                    })
                    .await?
                    .value;
                // An unknown session has no replays; an empty list says so.
                Some(found.unwrap_or((0, 0, 0)))
            }
            None => None,
        };
        let now = self.collector.clock().now_ms();
        let window = scope.share().map(|share| ShareWindow::of(share, now));
        let mut filter = Filter {
            start: window
                .as_ref()
                .map_or(args.start, |w| args.start.max(w.since)),
            end: args.end,
            cutoff: cutoff(ctx.conn(), &scope, now).await?,
            path: args.path.filter(|p| !p.is_empty()),
            device: args.device.filter(|d| !d.is_empty()),
            min_duration_ms: args.min_duration_ms,
            saved: args.saved.unwrap_or(false),
            session,
            before,
        };
        if let Some(pred) = matching {
            let hostnames = hostnames(ctx.conn(), &scope).await?;
            let retention = crate::retention::effective(
                &mut *ctx.conn(),
                crate::retention::Kind::Site,
                scope.site().as_str(),
            )
            .await?
            .ok_or_else(ApiError::not_found)?;
            let matching = SessionMatch {
                pred,
                hostnames,
                retention,
            };
            let data = self
                .filtered_page(site, filter, limit, window.as_ref(), matching)
                .await?;
            return Ok(Snapshot {
                topic: Topic::site(scope.site()),
                data,
            });
        }
        let (rows, next) = match &window {
            Some(window) => self.shared_page(site, &mut filter, limit, window).await?,
            None => {
                let mut rows = self.recordings.store.list(site, &filter, limit + 1).await?;
                let next = (rows.len() > limit as usize).then(|| {
                    rows.truncate(limit as usize);
                    let last = rows.last().expect("a full page");
                    next_cursor(last)
                });
                (rows, next)
            }
        };
        let links = self.session_links(site, &rows).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: RecordingList {
                captures: rows
                    .iter()
                    .zip(links)
                    .map(|(row, link)| summary(row, link))
                    .collect(),
                next,
                total: None,
            },
        })
    }

    async fn recordings_get(
        &self,
        ctx: &mut QueryCtx,
        args: RecordingsGetArgs,
    ) -> Result<Snapshot<ReplayPage>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead)
            .await?;
        let (site, row) = self
            .visible_capture(ctx.conn(), &scope, &args.capture)
            .await?;
        let after = match args.after.as_deref() {
            Some(cursor) => Position::parse(cursor)
                .ok_or_else(|| FieldErrors::one("after", FieldErrorCode::InvalidFormat, None))?,
            None => Position::START,
        };
        let capture = id_of(&row.capture);
        let (playback, next) =
            playback(self.recordings.store.clone(), site, capture, after).await?;
        let link = self
            .session_links(site, std::slice::from_ref(&row))
            .await?
            .pop()
            .flatten();
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: ReplayPage {
                capture: summary(&row, link),
                events: playback.events,
                timeline: playback.timeline,
                gaps: playback.gaps,
                next: next.map(Position::cursor),
            },
        })
    }

    async fn recordings_save(
        &self,
        ctx: &mut CommandCtx,
        args: CaptureArgs,
    ) -> Result<CaptureResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead, None)
            .await?;
        let (site, row) = self
            .visible_capture(ctx.conn(), &scope, &args.capture)
            .await?;
        let now = self.collector.clock().now_ms();
        let r = &self.recordings;
        let row = match r
            .store
            .save(site, id_of(&row.capture), now, r.saved_cap_bytes())
            .await?
        {
            Saved::Saved(row) => *row,
            Saved::NotFound => return Err(ApiError::not_found()),
            Saved::OverBudget => return Err(ApiError::new(ErrorCode::SavedReplayBudgetExceeded)),
        };
        ctx.record(
            &Topic::site(scope.site()),
            "site.replay_saved",
            json!({ "capture": args.capture }),
        )
        .await?;
        Ok(CaptureResult {
            capture: summary(&row, None),
        })
    }

    async fn recordings_unsave(
        &self,
        ctx: &mut CommandCtx,
        args: CaptureArgs,
    ) -> Result<CaptureResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead, None)
            .await?;
        let (site, row) = self
            .visible_capture(ctx.conn(), &scope, &args.capture)
            .await?;
        let row = self
            .recordings
            .store
            .unsave(site, id_of(&row.capture))
            .await?
            .ok_or_else(ApiError::not_found)?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.replay_unsaved",
            json!({ "capture": args.capture }),
        )
        .await?;
        Ok(CaptureResult {
            capture: summary(&row, None),
        })
    }

    async fn heatmaps_pages(
        &self,
        ctx: &mut QueryCtx,
        args: HeatmapPagesArgs,
    ) -> Result<Snapshot<HeatmapPages>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead)
            .await?;
        let (first, last) = self.heatmap_days(args.start, args.end)?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let pages = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                heatmaps::pages(s.connection(), site, first, last)
            })
            .await?
            .value;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: HeatmapPages { pages },
        })
    }

    async fn heatmaps_cells(
        &self,
        ctx: &mut QueryCtx,
        args: HeatmapCellsArgs,
    ) -> Result<Snapshot<HeatmapCells>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRecordingsRead)
            .await?;
        let (first, last) = self.heatmap_days(args.start, args.end)?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let (path, viewport) = (args.path, args.viewport);
        let cells = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                heatmaps::cells(s.connection(), site, &path, viewport, first, last)
            })
            .await?
            .value;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: cells,
        })
    }
}

impl AppState {
    /// A capture of the scoped site that is stored and inside retention, else `not_found`.
    async fn visible_capture(
        &self,
        conn: &mut SqliteConnection,
        scope: &SiteScope,
        capture: &CaptureId,
    ) -> Result<(u128, Capture), ApiError> {
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let capture = Uuid::try_parse(capture.as_str()).map_err(|_| ApiError::not_found())?;
        let row = self
            .recordings
            .store
            .capture(site, capture.as_u128())
            .await?
            .ok_or_else(ApiError::not_found)?;
        let now = self.collector.clock().now_ms();
        let cutoff = cutoff(conn, scope, now).await?;
        let shared = scope.share().map(|share| ShareWindow::of(share, now));
        if (row.saved_at.is_none() && row.received_at < cutoff)
            || shared.is_some_and(|window| !window.admits(&row))
        {
            return Err(ApiError::not_found());
        }
        Ok((site, row))
    }

    /// One page of a share viewer's list: scans newest first past replays outside the share
    /// until `limit` match, the site runs out, or [`SHARED_SCANS`] batches were read. `next`
    /// continues after the last replay scanned, so a page may hold fewer than `limit`.
    async fn shared_page(
        &self,
        site: u128,
        filter: &mut Filter,
        limit: u32,
        window: &ShareWindow,
    ) -> Result<(Vec<Capture>, Option<String>), ApiError> {
        let mut kept = Vec::new();
        let mut last = None;
        for _ in 0..SHARED_SCANS {
            let batch = self
                .recordings
                .store
                .list(site, filter, PAGE_SIZE_MAX)
                .await?;
            let exhausted = batch.len() < PAGE_SIZE_MAX as usize;
            for row in batch {
                filter.before = Some((row.received_at, id_of(&row.capture)));
                let cursor = next_cursor(&row);
                if window.admits(&row) {
                    kept.push(row);
                }
                if kept.len() == limit as usize {
                    return Ok((kept, Some(cursor)));
                }
                last = Some(cursor);
            }
            if exhausted {
                return Ok((kept, None));
            }
        }
        Ok((kept, last))
    }

    /// One page of a list with a filter: every replay the other arguments and the share
    /// admit, newest first, whose linked session has an event matching `pred`. Linking and
    /// matching run in one report-lane read, so the timeout bounds them together; `total`
    /// counts them all on the first page.
    async fn filtered_page(
        &self,
        site: u128,
        mut filter: Filter,
        limit: u32,
        window: Option<&ShareWindow>,
        matching: SessionMatch,
    ) -> Result<RecordingList, ApiError> {
        let SessionMatch {
            pred,
            hostnames,
            retention,
        } = matching;
        let before = filter.before.take();
        let mut rows = self.recordings.store.list(site, &filter, u32::MAX).await?;
        if let Some(window) = window {
            rows.retain(|row| window.admits(row));
        }
        let keys: Vec<(i64, u128, i64)> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                let visitor = row.visitor.as_deref()?;
                Some((i as i64, id_of(visitor), row.received_at))
            })
            .collect();
        let count = rows.len();
        let now_ms = self.collector.clock().now_ms();
        let links = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                let linked = linked_sessions(s.connection(), site, &keys)?;
                let starts = linked.iter().map(|&(_, _, start)| start);
                let span = match (starts.clone().min(), starts.max()) {
                    (Some(lo), Some(hi)) => (lo, hi + 1),
                    _ => (0, 0),
                };
                let context = Context {
                    site,
                    now_ms,
                    hostnames: &hostnames,
                    retention,
                };
                let matching: HashSet<i64> =
                    match explore::matching_sessions(s, &pred, &context, span)? {
                        Ok(sessions) => sessions.into_iter().collect(),
                        Err(refusal) => return Ok(Err(refusal)),
                    };
                let mut links = vec![None; count];
                for (i, session, _) in linked {
                    if matching.contains(&session) {
                        links[i as usize] = Some(session);
                    }
                }
                Ok(Ok(links))
            })
            .await?
            .value
            .map_err(refused)?;
        let matched: Vec<(Capture, i64)> = rows
            .into_iter()
            .zip(links)
            .filter_map(|(row, link)| Some((row, link?)))
            .collect();
        let total = before.is_none().then_some(matched.len() as i64);
        let mut page: Vec<(Capture, i64)> = matched
            .into_iter()
            .filter(|(row, _)| before.is_none_or(|at| (row.received_at, id_of(&row.capture)) < at))
            .take(limit as usize + 1)
            .collect();
        let next = (page.len() > limit as usize).then(|| {
            page.truncate(limit as usize);
            next_cursor(&page.last().expect("a full page").0)
        });
        Ok(RecordingList {
            captures: page
                .iter()
                .map(|(row, session)| summary(row, Some(*session)))
                .collect(),
            next,
            total,
        })
    }

    /// The analytical session that contains each capture's start under the current
    /// calculation. Captures only reference a visitor, so corrections never move them.
    async fn session_links(
        &self,
        site: u128,
        rows: &[Capture],
    ) -> Result<Vec<Option<i64>>, ApiError> {
        let keys: Vec<Option<(u128, i64)>> = rows
            .iter()
            .map(|row| {
                row.visitor
                    .as_deref()
                    .map(|visitor| (id_of(visitor), row.received_at))
            })
            .collect();
        if keys.iter().all(Option::is_none) {
            return Ok(vec![None; rows.len()]);
        }
        Ok(self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                let mut statement = s.connection().prepare(
                    "SELECT session_id FROM sessions
                     WHERE site = ? AND population = ? AND identity = ?
                       AND epoch_ms(start_time) - ? <= ? AND ? < epoch_ms(ends_at)
                     ORDER BY start_time DESC LIMIT 1",
                )?;
                keys.into_iter()
                    .map(|key| match key {
                        Some((visitor, at)) => statement
                            .query_row(
                                duckdb::params![
                                    site,
                                    POPULATION_DAILY,
                                    visitor,
                                    SESSION_SLACK_MS,
                                    at,
                                    at
                                ],
                                |row| row.get(0),
                            )
                            .optional(),
                        None => Ok(None),
                    })
                    .collect()
            })
            .await?
            .value)
    }

    /// Requested UTC days, clamped to heatmap retention.
    fn heatmap_days(&self, start: i64, end: i64) -> Result<(i32, i32), ApiError> {
        let mut errors = FieldErrors::default();
        range(start, end, &mut errors);
        errors.finish()?;
        let first = heatmaps::utc_day(start).max(heatmaps::first_retained_day(
            self.collector.clock().now_ms(),
        ));
        Ok((first, heatmaps::utc_day(end - 1)))
    }
}

/// For each `(index, visitor, received_at)`, the daily session containing it as in
/// [`AppState::session_links`]: `(index, session, session start)`, for linked keys only.
fn linked_sessions(
    conn: &Connection,
    site: u128,
    keys: &[(i64, u128, i64)],
) -> duckdb::Result<Vec<(i64, i64, i64)>> {
    conn.execute_batch(
        "CREATE OR REPLACE TEMP TABLE rp_keys (idx BIGINT, visitor UHUGEINT, received BIGINT)",
    )?;
    {
        let mut appender = conn.appender_to_catalog_and_db("rp_keys", "temp", "main")?;
        for &(index, visitor, at) in keys {
            appender.append_row(duckdb::params![index, visitor, at])?;
        }
        appender.flush()?;
    }
    let linked = conn
        .prepare(
            "SELECT k.idx, arg_max(s.session_id, s.start_time), max(epoch_ms(s.start_time))
             FROM rp_keys k JOIN sessions s
               ON s.site = ? AND s.population = ? AND s.identity = k.visitor
              AND epoch_ms(s.start_time) - ? <= k.received AND k.received < epoch_ms(s.ends_at)
             GROUP BY k.idx",
        )?
        .query_map(
            duckdb::params![site, POPULATION_DAILY, SESSION_SLACK_MS],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
        .collect();
    conn.execute_batch("DROP TABLE rp_keys")?;
    linked
}

/// The visitor and receive window of one daily analytical session.
fn session_window(
    conn: &Connection,
    site: u128,
    session: i64,
) -> duckdb::Result<Option<(u128, i64, i64)>> {
    conn.query_row(
        "SELECT identity, epoch_ms(start_time), epoch_ms(ends_at) FROM sessions
         WHERE site = ? AND session_id = ? AND population = ?",
        duckdb::params![site, session, POPULATION_DAILY],
        |row| {
            Ok((
                row.get(0)?,
                row.get::<_, i64>(1)? - SESSION_SLACK_MS,
                row.get(2)?,
            ))
        },
    )
    .optional()
}

fn range(start: i64, end: i64, errors: &mut FieldErrors) {
    let in_range = |ms: i64| (MIN_TIME_MS..=MAX_TIME_MS).contains(&ms);
    if !in_range(start) {
        errors.add("start", FieldErrorCode::InvalidValue, None);
    }
    if !in_range(end) || end <= start {
        errors.add("end", FieldErrorCode::InvalidValue, None);
    }
}

/// `<received_at>:<capture>` of the last listed capture.
fn next_cursor(row: &Capture) -> String {
    format!(
        "{}:{}",
        row.received_at,
        Uuid::from_u128(id_of(&row.capture)).simple()
    )
}

fn cursor(text: &str) -> Option<(i64, u128)> {
    let (at, capture) = text.split_once(':')?;
    Some((at.parse().ok()?, Uuid::try_parse(capture).ok()?.as_u128()))
}

async fn cutoff(conn: &mut SqliteConnection, scope: &SiteScope, now: i64) -> Result<i64, ApiError> {
    let policies = session_policies(conn, scope.site().as_str())
        .await?
        .ok_or_else(|| ApiError::internal("site has no session policy"))?;
    Ok(replay_cutoff_ms(policies.at(now).policy.timezone(), now))
}

fn summary(row: &Capture, session: Option<i64>) -> CaptureSummary {
    CaptureSummary {
        id: CaptureId::new(
            Uuid::from_u128(id_of(&row.capture))
                .hyphenated()
                .to_string(),
        ),
        received_at: row.received_at,
        duration_ms: (row.ended_at - row.started_at).max(0),
        events: row.events,
        bytes: row.bytes,
        entry_path: row.entry_path.clone(),
        pages: row.pages,
        browser: row.browser.clone(),
        os: row.os.clone(),
        device: row.device.clone(),
        country: row.country.clone(),
        saved: row.saved_at.is_some(),
        truncated: row.truncated,
        session,
    }
}

/// Reads one playback page on a thread with a stack for deeply nested pages. Parts are read in
/// order after `after` until the page holds [`PAGE_BYTES`] and no split event is open, or
/// [`PAGE_HARD_BYTES`] of input were decoded. Returns the page and the cursor of the next one.
async fn playback(
    store: Store,
    site: u128,
    capture: u128,
    after: Position,
) -> Result<(super::replay::Playback, Option<Position>), ApiError> {
    let runtime = tokio::runtime::Handle::current();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("replay-playback".into())
        .stack_size(PLAYBACK_STACK_BYTES)
        .spawn(move || {
            let read = || -> Result<_, sqlx::Error> {
                let mut assembler = Assembler::default();
                let (mut cursor, mut decoded, mut next) = (after, 0i64, None);
                'parts: loop {
                    let parts =
                        runtime.block_on(store.parts(site, capture, cursor, PARTS_PER_READ))?;
                    if parts.is_empty() {
                        break;
                    }
                    for (position, raw_bytes, payload) in parts {
                        let full = assembler.bytes() >= PAGE_BYTES && !assembler.has_pending();
                        if full || decoded >= PAGE_HARD_BYTES {
                            next = Some(cursor);
                            break 'parts;
                        }
                        match zstd::bulk::decompress(&payload, raw_bytes.max(0) as usize) {
                            Ok(json) => assembler.push(&json),
                            Err(_) => assembler.damaged(),
                        }
                        decoded += raw_bytes;
                        cursor = position;
                    }
                }
                Ok((assembler.finish(), next))
            };
            let _ = sender.send(read());
        })
        .map_err(ApiError::internal)?;
    receiver
        .await
        .map_err(|_| ApiError::internal("playback thread ended"))?
        .map_err(ApiError::from)
}

/// What a listed replay's session must match, with the site facts the match reads: its
/// hostnames for the internal-referrer rule and its event retention (0016).
struct SessionMatch {
    pred: Pred,
    hostnames: Vec<String>,
    retention: holocron_protocol::retention::Retention,
}
