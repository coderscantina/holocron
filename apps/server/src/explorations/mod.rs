//! Exploration of one site: `analytics.explore` and `analytics.export`, the field catalog,
//! realtime, and the site's saved explorations, segments and annotations. Boards live in
//! [`boards`], the live subscription in [`live`].
//!
//! Every read resolves the caller's site scope first, so a revoked scope is refused before any
//! analytical work, and exports need `site.export` on top of `site.read`.

mod boards;
mod export;
mod goals;
pub mod live;

use holocron_analytics::explore::{self, Context, Query, Refusal};
use holocron_analytics::{catalog, freshness, realtime};
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::{ErrorCode, FieldError, FieldErrorCode};
use holocron_protocol::exploration::{
    Cell, Compare, ExplorationState, ExploreResult, Field, Granularity, Group, Measure, Metric,
    NamedMeasure, Page, Panel, PropertyScope, Range, RangeUnit, Sort, SortDir, SortKey,
    Visualization,
};
use holocron_protocol::ids::{AnnotationId, ExplorationId, SegmentId, SiteId};
use holocron_protocol::limits::{
    ANNOTATION_TEXT_MAX_CHARS, DEFINITION_NAME_MAX_CHARS, EXPLORATION_ROWS_MAX,
    SESSION_TIMELINE_HOURS_MAX,
};
use holocron_protocol::operations::explorations::{
    ActiveLocation, Annotation, AnnotationList, AnnotationResult, AnnotationsCreateArgs,
    AnnotationsDeleteArgs, AnnotationsListArgs, AnnotationsUpdateArgs, Deleted,
    ExplorationsGetArgs, ExplorationsHandlers, ExplorationsSaveArgs, ExploreArgs, ExportArgs,
    ExportFile, FieldCatalog, FieldValue, FieldValues, FieldsCatalogArgs, FieldsValuesArgs,
    Processing, PropertyKey, PropertyType, RealtimeArgs, RealtimeBucket, RealtimeSnapshot,
    RecentEvent, RecentKind, SavedExploration, SavedExplorationList, SavedExplorationResult,
    Segment, SegmentList, SegmentResult, SegmentsDeleteArgs, SegmentsSaveArgs, SessionDetail,
    SessionEvent, SessionFields, SessionProperty, SessionTimeline, SessionsGetArgs,
    SessionsTimelineArgs, SiteArgs, TimelineSession,
};
use holocron_protocol::operations::sharing::ExplorationLimits;
use holocron_protocol::registry::Snapshot;
use holocron_protocol::retention::Retention;
use holocron_storage::Lane;
use serde::de::DeserializeOwned;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::maintenance::health;
use crate::sharing;
use crate::sites::policy::analytics_id;
use crate::sites::scope::SiteScope;
use crate::state::AppState;

/// Value suggestions look back this far.
const SUGGESTION_DAYS: u16 = 30;
const SUGGESTION_WINDOW_MS: i64 = SUGGESTION_DAYS as i64 * 86_400_000;
const SUGGESTIONS: usize = 20;
/// Rows a share viewer's suggestions are picked from, before the prefix applies.
const SHARED_SUGGESTION_ROWS: u32 = 100;
/// The realtime freshness covers the realtime series.
const REALTIME_WINDOW_MS: i64 = 30 * 60_000;

/// A planner refusal as the API error naming its field.
pub(crate) fn refused(refusal: Refusal) -> ApiError {
    ApiError::field(
        refusal.code,
        &refusal.field,
        FieldError {
            code: refusal.reason,
            limit: refusal.limit,
        },
        refusal.message,
    )
}

/// Tenths of a degree, as stored, to degrees on the wire.
fn degrees(tenths: i16) -> f64 {
    f64::from(tenths) / 10.0
}

/// JSON text arguments that do not parse are malformed requests, with serde's reason.
pub(crate) fn parse<T: DeserializeOwned>(text: &str, argument: &str) -> Result<T, ApiError> {
    serde_json::from_str(text).map_err(|error| {
        ApiError::with_message(ErrorCode::MalformedRequest, format!("{argument}: {error}"))
    })
}

/// A trimmed name of 1 to 100 characters.
pub(crate) fn definition_name(name: &str, field: &str) -> Result<String, ApiError> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 {
        return Err(FieldErrors::one(field, FieldErrorCode::Required, None));
    }
    if length > DEFINITION_NAME_MAX_CHARS {
        return Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(DEFINITION_NAME_MAX_CHARS as u32),
        ));
    }
    Ok(name.to_owned())
}

/// Checks a state for storage: the planner's checks, and the site it must belong to.
pub(crate) fn checked_state(state: &ExplorationState, site: &SiteId) -> Result<Query, ApiError> {
    if state.site != *site {
        return Err(FieldErrors::one(
            "state",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    explore::validate(state, &[]).map_err(refused)
}

pub(crate) async fn hostnames(
    conn: &mut SqliteConnection,
    scope: &SiteScope,
) -> Result<Vec<String>, ApiError> {
    let domain: String = sqlx::query_scalar("SELECT domain FROM sites WHERE id = ?")
        .bind(scope.site().as_str())
        .fetch_one(conn)
        .await?;
    Ok(vec![domain])
}

impl AppState {
    /// Runs a checked query for an authorized scope on the report lane.
    pub(crate) async fn run_exploration(
        &self,
        conn: &mut SqliteConnection,
        scope: &SiteScope,
        query: Query,
    ) -> Result<ExploreResult, ApiError> {
        let hostnames = hostnames(conn, scope).await?;
        let retention = crate::retention::effective(
            &mut *conn,
            crate::retention::Kind::Site,
            scope.site().as_str(),
        )
        .await?
        .ok_or_else(ApiError::not_found)?;
        self.explore_source(scope.site().as_str(), hostnames, retention, query, true)
            .await
    }

    /// Runs a checked query of an analytical source the caller may read, on the report lane.
    /// Only sites have rollups; links and pixels pass `rollups: false`.
    pub(crate) async fn explore_source(
        &self,
        source: &str,
        hostnames: Vec<String>,
        retention: Retention,
        query: Query,
        rollups: bool,
    ) -> Result<ExploreResult, ApiError> {
        let site = analytics_id(source).map_err(ApiError::internal)?;
        let now_ms = self.collector.clock().now_ms();
        let storage = self.collector.storage();
        let read = storage
            .read(Lane::Report, move |snapshot| {
                let context = Context {
                    site,
                    now_ms,
                    hostnames: &hostnames,
                    retention,
                };
                if rollups {
                    explore::explore(snapshot, &query, &context)
                } else {
                    explore::explore_raw(snapshot, &query, &context)
                }
            })
            .await?;
        let mut result = read.value.map_err(refused)?;
        health::mark_behind(storage, read.watermark, now_ms, &mut result.freshness)
            .await
            .map_err(ApiError::internal)?;
        Ok(result)
    }
}

impl AppState {
    /// Value suggestions for a share viewer: the top values of a granted field inside the
    /// share's pinned filter over the last 30 days or its range limit, whichever is shorter.
    /// Counted as an exploration, so the share's limits hold; the counts show only when the
    /// share grants `events`.
    async fn shared_values(
        &self,
        conn: &mut SqliteConnection,
        scope: &SiteScope,
        shared: &ExplorationState,
        limits: &ExplorationLimits,
        field: Field,
        prefix: Option<String>,
    ) -> Result<Vec<FieldValue>, ApiError> {
        let events = Measure::Named(NamedMeasure::Events);
        let state = ExplorationState {
            measures: vec![events.clone()],
            dimensions: vec![field],
            filter: limits.pinned_filter.clone(),
            range: Range::Relative {
                unit: RangeUnit::Day,
                count: limits.range_max_days.min(SUGGESTION_DAYS),
            },
            granularity: Granularity::Day,
            compare: Compare::None,
            sort: vec![Sort {
                by: SortKey::Measure(events),
                dir: SortDir::Desc,
            }],
            page: Page {
                size: SHARED_SUGGESTION_ROWS,
                index: 0,
            },
            visualization: Visualization::Table,
            report: None,
            ..shared.clone()
        };
        let query = explore::validate(&state, &[]).map_err(refused)?;
        let result = self.run_exploration(conn, scope, query).await?;
        let counted = limits.measures.contains(&NamedMeasure::Events);
        let prefix = prefix.unwrap_or_default().to_lowercase();
        Ok(result
            .main
            .rows
            .into_iter()
            .filter_map(|mut row| {
                let value = row.key.pop()?;
                let text = match &value {
                    Cell::String(text) | Cell::Decimal(text) | Cell::Date(text) => text.clone(),
                    Cell::Integer(n) => n.to_string(),
                    Cell::Bool(b) => b.to_string(),
                    Cell::Null | Cell::NotSet => String::new(),
                };
                if !text.to_lowercase().starts_with(&prefix) {
                    return None;
                }
                let events = match row.values.first() {
                    Some(Metric::Count { v }) if counted => *v,
                    _ => 0,
                };
                Some(FieldValue { value, events })
            })
            .take(SUGGESTIONS)
            .collect())
    }
}

impl ExplorationsHandlers for AppState {
    async fn analytics_explore(
        &self,
        ctx: &mut QueryCtx,
        args: ExploreArgs,
    ) -> Result<Snapshot<ExploreResult>, ApiError> {
        let state: ExplorationState = parse(&args.state, "state")?;
        let panels = args
            .panels
            .iter()
            .map(|panel| parse::<Panel>(panel, "panels"))
            .collect::<Result<Vec<_>, _>>()?;
        let scope = ctx
            .state_scope(&state, &panels, Capability::SiteRead)
            .await?;
        let query = explore::validate(&state, &panels).map_err(refused)?;
        let result = self.run_exploration(ctx.conn(), &scope, query).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: result,
        })
    }

    async fn analytics_export(
        &self,
        ctx: &mut QueryCtx,
        args: ExportArgs,
    ) -> Result<Snapshot<ExportFile>, ApiError> {
        let state: ExplorationState = parse(&args.state, "state")?;
        let scope = ctx.state_scope(&state, &[], Capability::SiteExport).await?;
        let mut query = explore::validate(&state, &[]).map_err(refused)?;
        // Every addressable row, not one page: the export is the whole result of the state.
        query.main.offset = 0;
        query.main.size = EXPLORATION_ROWS_MAX;
        let domain = hostnames(ctx.conn(), &scope).await?.remove(0);
        let result = self.run_exploration(ctx.conn(), &scope, query).await?;
        let file = export::file(
            &state,
            &result,
            args.format,
            &domain,
            self.collector.clock().now_ms(),
        )
        .map_err(ApiError::internal)?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: file,
        })
    }

    async fn fields_catalog(
        &self,
        ctx: &mut QueryCtx,
        args: FieldsCatalogArgs,
    ) -> Result<Snapshot<FieldCatalog>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let keys = self
            .collector
            .storage()
            .read(Lane::Report, move |s| catalog::property_keys(s, site))
            .await?
            .value;
        let properties = keys
            .into_iter()
            .map(|key| PropertyKey {
                scope: key.scope,
                key: key.key,
                types: key.types.into_iter().filter_map(property_type).collect(),
                rows: key.rows,
            })
            .collect();
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: FieldCatalog { properties },
        })
    }

    async fn fields_values(
        &self,
        ctx: &mut QueryCtx,
        args: FieldsValuesArgs,
    ) -> Result<Snapshot<FieldValues>, ApiError> {
        let field: Field = parse(&args.field, "field")?;
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        if let Field::Property(property) = &field {
            let length = property.key.len();
            if length == 0 || length > holocron_protocol::limits::EXPLORATION_PROPERTY_KEY_MAX_BYTES
            {
                return Err(FieldErrors::one(
                    "field",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        }
        if let Some(share) = scope.share() {
            let (shared, limits) = sharing::admit_field(share, &field)?;
            let values = self
                .shared_values(ctx.conn(), &scope, shared, limits, field, args.prefix)
                .await?;
            return Ok(Snapshot {
                topic: Topic::site(scope.site()),
                data: FieldValues { values },
            });
        }
        let hostnames = hostnames(ctx.conn(), &scope).await?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let now = self.collector.clock().now_ms();
        let prefix = args.prefix;
        let values = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                explore::top_values(
                    s,
                    site,
                    &field,
                    prefix.as_deref(),
                    (now - SUGGESTION_WINDOW_MS, now + 1),
                    &hostnames,
                    SUGGESTIONS,
                )
            })
            .await?
            .value;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: FieldValues {
                values: values
                    .into_iter()
                    .map(|(value, events)| FieldValue { value, events })
                    .collect(),
            },
        })
    }

    async fn sessions_get(
        &self,
        ctx: &mut QueryCtx,
        args: SessionsGetArgs,
    ) -> Result<Snapshot<SessionDetail>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let hostnames = hostnames(ctx.conn(), &scope).await?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let id = args.session;
        let detail = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                explore::session::session(s, site, id, &hostnames)
            })
            .await?
            .value
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound))?;
        let mut fields = SessionFields::default();
        for (name, value) in detail.fields {
            let slot = match name {
                "channel" => &mut fields.channel,
                "referrer_domain" => &mut fields.referrer_domain,
                "referrer_path" => &mut fields.referrer_path,
                "utm_source" => &mut fields.utm_source,
                "utm_medium" => &mut fields.utm_medium,
                "utm_campaign" => &mut fields.utm_campaign,
                "country" => &mut fields.country,
                "region" => &mut fields.region,
                "city" => &mut fields.city,
                "device" => &mut fields.device,
                "browser" => &mut fields.browser,
                "os" => &mut fields.os,
                "language" => &mut fields.language,
                "screen" => &mut fields.screen,
                _ => continue,
            };
            *slot = value;
        }
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SessionDetail {
                session: detail.id,
                person: detail.person,
                start: detail.start,
                end: detail.last,
                pageviews: detail.pageviews,
                custom_events: detail.custom_events,
                fields,
                events: detail.events.into_iter().map(wire_event).collect(),
                truncated: detail.truncated,
                properties: detail
                    .properties
                    .into_iter()
                    .map(|(key, value)| SessionProperty { key, value })
                    .collect(),
            },
        })
    }

    async fn sessions_timeline(
        &self,
        ctx: &mut QueryCtx,
        args: SessionsTimelineArgs,
    ) -> Result<Snapshot<SessionTimeline>, ApiError> {
        if !(1..=SESSION_TIMELINE_HOURS_MAX).contains(&args.hours) {
            return Err(FieldErrors::one(
                "hours",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let to = self.collector.clock().now_ms() + 1;
        let from = to - i64::from(args.hours) * 3_600_000;
        let timeline = self
            .collector
            .storage()
            .read(Lane::Report, move |s| {
                explore::session::timeline(s, site, from, to)
            })
            .await?
            .value;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SessionTimeline {
                from,
                to,
                total: timeline.total,
                median_span: timeline.median_span,
                sessions: timeline
                    .sessions
                    .into_iter()
                    .map(|s| TimelineSession {
                        session: s.id,
                        person: s.person,
                        start: s.start,
                        end: s.last,
                        pageviews: s.pageviews,
                        custom_events: s.custom_events,
                        country: s.country,
                        device: s.device,
                        browser: s.browser,
                        events: s.events.into_iter().map(wire_event).collect(),
                        truncated: s.truncated,
                    })
                    .collect(),
            },
        })
    }

    async fn realtime_snapshot(
        &self,
        ctx: &mut QueryCtx,
        args: RealtimeArgs,
    ) -> Result<Snapshot<RealtimeSnapshot>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let now = self.collector.clock().now_ms();
        let storage = self.collector.storage();
        let read = storage
            .read(Lane::Live, move |s| {
                let live = realtime::realtime(s, site, now)?;
                let fresh = freshness(s, site, now - REALTIME_WINDOW_MS, now + 1)?;
                Ok((live, fresh))
            })
            .await?;
        let (live, fresh) = read.value;
        let mut freshness = explore::wire_freshness(fresh, now);
        health::mark_behind(storage, read.watermark, now, &mut freshness)
            .await
            .map_err(ApiError::internal)?;
        let status = storage.status();
        let inbox_age_ms = storage
            .inbox_age()
            .await
            .map_err(ApiError::internal)?
            .as_millis() as i64;
        let topic = live::topic(scope.site());
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: RealtimeSnapshot {
                now,
                active: live.active,
                series: live
                    .series
                    .into_iter()
                    .map(|(start, pageviews, visitors)| RealtimeBucket {
                        start,
                        pageviews,
                        visitors,
                    })
                    .collect(),
                recent: live
                    .recent
                    .into_iter()
                    .map(|r| RecentEvent {
                        at: r.at,
                        kind: if r.pageview {
                            RecentKind::Pageview
                        } else {
                            RecentKind::CustomEvent
                        },
                        path: r.path,
                        event_name: r.event_name,
                        country: r.country,
                        city: r.city,
                        lat: r.lat.map(degrees),
                        lng: r.lng.map(degrees),
                        device: r.device,
                        browser: r.browser,
                    })
                    .collect(),
                active_locations: live
                    .locations
                    .into_iter()
                    .map(|l| ActiveLocation {
                        lat: degrees(l.lat),
                        lng: degrees(l.lng),
                        country: l.country,
                        city: l.city,
                        visitors: l.visitors,
                        path: l.path,
                    })
                    .collect(),
                identity_coverage: holocron_protocol::exploration::Coverage {
                    with_identity: live.with_identity,
                    eligible_events: live.eligible,
                },
                freshness,
                processing: Processing {
                    inbox_records: status.inbox_records as i64,
                    inbox_age_ms,
                },
                live_cursor: live::cursor(&topic, read.watermark.batch),
                live_topic: topic,
            },
        })
    }

    async fn explorations_list(
        &self,
        ctx: &mut QueryCtx,
        args: SiteArgs,
    ) -> Result<Snapshot<SavedExplorationList>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let user = ctx.person()?;
        let rows: Vec<ExplorationRow> = sqlx::query_as(
            "SELECT id, site_id, name, state, created_at, updated_at FROM saved_explorations
             WHERE owner_id = ? AND site_id = ? ORDER BY updated_at DESC, id",
        )
        .bind(user.as_str())
        .bind(scope.site().as_str())
        .fetch_all(ctx.conn())
        .await?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: SavedExplorationList {
                explorations: rows.into_iter().map(saved).collect::<Result<_, _>>()?,
            },
        })
    }

    async fn explorations_get(
        &self,
        ctx: &mut QueryCtx,
        args: ExplorationsGetArgs,
    ) -> Result<Snapshot<SavedExplorationResult>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let user = ctx.person()?;
        let exploration = own_exploration(ctx.conn(), &user, &scope, &args.exploration).await?;
        Ok(Snapshot {
            topic: Topic::user(&user),
            data: SavedExplorationResult { exploration },
        })
    }

    async fn explorations_save(
        &self,
        ctx: &mut CommandCtx,
        args: ExplorationsSaveArgs,
    ) -> Result<SavedExplorationResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRead, None)
            .await?;
        let user = ctx.person()?;
        let name = definition_name(&args.name, "name")?;
        checked_state(&args.state, scope.site())?;
        let state = serde_json::to_string(&args.state).map_err(ApiError::internal)?;
        let now = ctx.now();
        let id = match &args.exploration {
            Some(id) => {
                own_exploration(ctx.conn(), &user, &scope, id).await?;
                sqlx::query("UPDATE saved_explorations SET name = ?, state = ?, updated_at = ? WHERE id = ?")
                    .bind(&name)
                    .bind(&state)
                    .bind(now)
                    .bind(id.as_str())
                    .execute(ctx.conn())
                    .await?;
                id.clone()
            }
            None => {
                let id = ExplorationId::new(crate::db::new_id());
                sqlx::query(
                    "INSERT INTO saved_explorations (id, site_id, owner_id, name, state, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(id.as_str())
                .bind(scope.site().as_str())
                .bind(user.as_str())
                .bind(&name)
                .bind(&state)
                .bind(now)
                .bind(now)
                .execute(ctx.conn())
                .await?;
                id
            }
        };
        ctx.record(
            &Topic::user(&user),
            "exploration.saved",
            json!({ "id": id, "site": scope.site() }),
        )
        .await?;
        let exploration = own_exploration(ctx.conn(), &user, &scope, &id).await?;
        Ok(SavedExplorationResult { exploration })
    }

    async fn explorations_delete(
        &self,
        ctx: &mut CommandCtx,
        args: ExplorationsGetArgs,
    ) -> Result<Deleted, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteRead, None)
            .await?;
        let user = ctx.person()?;
        own_exploration(ctx.conn(), &user, &scope, &args.exploration).await?;
        sqlx::query("DELETE FROM saved_explorations WHERE id = ?")
            .bind(args.exploration.as_str())
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::user(&user),
            "exploration.deleted",
            json!({ "id": args.exploration }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn segments_list(
        &self,
        ctx: &mut QueryCtx,
        args: SiteArgs,
    ) -> Result<Snapshot<SegmentList>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, name, filter, created_at FROM segments WHERE site_id = ? ORDER BY name_key, id",
        )
        .bind(scope.site().as_str())
        .fetch_all(ctx.conn())
        .await?;
        let segments = rows
            .into_iter()
            .map(|(id, name, filter, created_at)| {
                Ok(Segment {
                    id: SegmentId::new(id),
                    site: scope.site().clone(),
                    name,
                    filter: serde_json::from_str(&filter).map_err(ApiError::internal)?,
                    created_at,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: SegmentList { segments },
        })
    }

    async fn segments_save(
        &self,
        ctx: &mut CommandCtx,
        args: SegmentsSaveArgs,
    ) -> Result<SegmentResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let user = ctx.person()?;
        let name = definition_name(&args.name, "name")?;
        check_filter(&args.filter, scope.site())?;
        let key = name.to_lowercase();
        let taken: Option<String> =
            sqlx::query_scalar("SELECT id FROM segments WHERE site_id = ? AND name_key = ?")
                .bind(scope.site().as_str())
                .bind(&key)
                .fetch_optional(ctx.conn())
                .await?;
        if taken.is_some() {
            return Err(FieldErrors::one("name", FieldErrorCode::InvalidValue, None));
        }
        let id = SegmentId::new(crate::db::new_id());
        let now = ctx.now();
        sqlx::query(
            "INSERT INTO segments (id, site_id, name, name_key, filter, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(scope.site().as_str())
        .bind(&name)
        .bind(&key)
        .bind(serde_json::to_string(&args.filter).map_err(ApiError::internal)?)
        .bind(user.as_str())
        .bind(now)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "segment.saved",
            json!({ "id": id, "name": name }),
        )
        .await?;
        Ok(SegmentResult {
            segment: Segment {
                id,
                site: scope.site().clone(),
                name,
                filter: args.filter,
                created_at: now,
            },
        })
    }

    async fn segments_delete(
        &self,
        ctx: &mut CommandCtx,
        args: SegmentsDeleteArgs,
    ) -> Result<Deleted, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let deleted = sqlx::query("DELETE FROM segments WHERE id = ? AND site_id = ?")
            .bind(args.segment.as_str())
            .bind(scope.site().as_str())
            .execute(ctx.conn())
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(ApiError::not_found());
        }
        ctx.record(
            &Topic::site(scope.site()),
            "segment.deleted",
            json!({ "id": args.segment }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }

    async fn annotations_list(
        &self,
        ctx: &mut QueryCtx,
        args: AnnotationsListArgs,
    ) -> Result<Snapshot<AnnotationList>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        if args.end <= args.start {
            return Err(FieldErrors::one("end", FieldErrorCode::InvalidValue, None));
        }
        let rows: Vec<(String, i64, String, i64)> = sqlx::query_as(
            "SELECT id, at, text, created_at FROM annotations WHERE site_id = ? AND at >= ? AND at < ? ORDER BY at, id LIMIT 500",
        )
        .bind(scope.site().as_str())
        .bind(args.start)
        .bind(args.end)
        .fetch_all(ctx.conn())
        .await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: AnnotationList {
                annotations: rows
                    .into_iter()
                    .map(|(id, at, text, created_at)| Annotation {
                        id: AnnotationId::new(id),
                        site: scope.site().clone(),
                        at,
                        text,
                        created_at,
                    })
                    .collect(),
            },
        })
    }

    async fn annotations_create(
        &self,
        ctx: &mut CommandCtx,
        args: AnnotationsCreateArgs,
    ) -> Result<AnnotationResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let user = ctx.person()?;
        let text = annotation_text(&args.text)?;
        let id = AnnotationId::new(crate::db::new_id());
        let now = ctx.now();
        sqlx::query("INSERT INTO annotations (id, site_id, at, text, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id.as_str())
            .bind(scope.site().as_str())
            .bind(args.at)
            .bind(&text)
            .bind(user.as_str())
            .bind(now)
            .execute(ctx.conn())
            .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "annotation.created",
            json!({ "id": id }),
        )
        .await?;
        Ok(AnnotationResult {
            annotation: Annotation {
                id,
                site: scope.site().clone(),
                at: args.at,
                text,
                created_at: now,
            },
        })
    }

    async fn annotations_update(
        &self,
        ctx: &mut CommandCtx,
        args: AnnotationsUpdateArgs,
    ) -> Result<AnnotationResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let text = annotation_text(&args.text)?;
        let created_at: Option<i64> = sqlx::query_scalar(
            "UPDATE annotations SET at = ?, text = ? WHERE id = ? AND site_id = ? RETURNING created_at",
        )
        .bind(args.at)
        .bind(&text)
        .bind(args.annotation.as_str())
        .bind(scope.site().as_str())
        .fetch_optional(ctx.conn())
        .await?;
        let created_at = created_at.ok_or_else(ApiError::not_found)?;
        ctx.record(
            &Topic::site(scope.site()),
            "annotation.updated",
            json!({ "id": args.annotation }),
        )
        .await?;
        Ok(AnnotationResult {
            annotation: Annotation {
                id: args.annotation,
                site: scope.site().clone(),
                at: args.at,
                text,
                created_at,
            },
        })
    }

    async fn annotations_delete(
        &self,
        ctx: &mut CommandCtx,
        args: AnnotationsDeleteArgs,
    ) -> Result<Deleted, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteDefinitionsWrite, None)
            .await?;
        let deleted = sqlx::query("DELETE FROM annotations WHERE id = ? AND site_id = ?")
            .bind(args.annotation.as_str())
            .bind(scope.site().as_str())
            .execute(ctx.conn())
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(ApiError::not_found());
        }
        ctx.record(
            &Topic::site(scope.site()),
            "annotation.deleted",
            json!({ "id": args.annotation }),
        )
        .await?;
        Ok(Deleted { deleted: true })
    }
}

fn annotation_text(text: &str) -> Result<String, ApiError> {
    let text = text.trim();
    let length = text.chars().count();
    if length == 0 {
        return Err(FieldErrors::one("text", FieldErrorCode::Required, None));
    }
    if length > ANNOTATION_TEXT_MAX_CHARS {
        return Err(FieldErrors::one(
            "text",
            FieldErrorCode::TooLong,
            Some(ANNOTATION_TEXT_MAX_CHARS as u32),
        ));
    }
    Ok(text.to_owned())
}

/// A segment's filter passes the same checks as a filter inside a state.
fn check_filter(filter: &Group, site: &SiteId) -> Result<(), ApiError> {
    let state = ExplorationState {
        v: holocron_protocol::exploration::STATE_VERSION,
        site: site.clone(),
        measures: vec![Measure::Named(NamedMeasure::Pageviews)],
        dimensions: Vec::new(),
        filter: Some(filter.clone()),
        range: holocron_protocol::exploration::Range::Relative {
            unit: holocron_protocol::exploration::RangeUnit::Day,
            count: 1,
        },
        timezone: "UTC".into(),
        granularity: holocron_protocol::exploration::Granularity::Day,
        compare: holocron_protocol::exploration::Compare::None,
        metric_version: holocron_analytics::METRIC_VERSION,
        sort: Vec::new(),
        page: explore::DEFAULT_PAGE,
        visualization: holocron_protocol::exploration::Visualization::Table,
        report: None,
    };
    explore::validate(&state, &[]).map(drop).map_err(refused)
}

type ExplorationRow = (String, String, String, String, i64, i64);

fn saved(
    (id, site, name, state, created_at, updated_at): ExplorationRow,
) -> Result<SavedExploration, ApiError> {
    Ok(SavedExploration {
        id: ExplorationId::new(id),
        site: SiteId::new(site),
        name,
        state: serde_json::from_str(&state).map_err(ApiError::internal)?,
        created_at,
        updated_at,
    })
}

/// One of the caller's saved explorations on the scope's site; anything else is not found.
async fn own_exploration(
    conn: &mut SqliteConnection,
    user: &holocron_protocol::ids::UserId,
    scope: &SiteScope,
    id: &ExplorationId,
) -> Result<SavedExploration, ApiError> {
    let row: Option<ExplorationRow> = sqlx::query_as(
        "SELECT id, site_id, name, state, created_at, updated_at FROM saved_explorations
         WHERE id = ? AND owner_id = ? AND site_id = ?",
    )
    .bind(id.as_str())
    .bind(user.as_str())
    .bind(scope.site().as_str())
    .fetch_optional(conn)
    .await?;
    saved(row.ok_or_else(ApiError::not_found)?)
}

fn property_type(tag: u8) -> Option<PropertyType> {
    use holocron_storage::property_type as t;
    Some(match tag {
        t::NULL => PropertyType::Null,
        t::BOOL => PropertyType::Bool,
        t::INTEGER => PropertyType::Integer,
        t::DECIMAL => PropertyType::Decimal,
        t::DATE => PropertyType::Date,
        t::STRING => PropertyType::String,
        t::ARRAY => PropertyType::Array,
        _ => return None,
    })
}

/// The scope a property field is read in, for the catalog's label.
pub(crate) fn scope_name(scope: PropertyScope) -> &'static str {
    match scope {
        PropertyScope::Event => "event",
        PropertyScope::Session => "session",
    }
}

fn wire_event(e: explore::session::SessionEvent) -> SessionEvent {
    SessionEvent {
        at: e.at,
        kind: if e.pageview {
            RecentKind::Pageview
        } else {
            RecentKind::CustomEvent
        },
        path: e.path,
        title: e.title,
        event_name: e.event_name,
    }
}
