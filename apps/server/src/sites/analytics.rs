//! `recalculations.*` and `analytics.freshness`: correction state of a site's derived data.

use chrono::NaiveDate;
use holocron_analytics::explore::wire_freshness;
use holocron_analytics::identity::{MAX_TIME_MS, MIN_TIME_MS};
use holocron_analytics::{freshness, jobs};
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::FieldErrorCode;
use holocron_protocol::ids::RecalculationId;
use holocron_protocol::limits::RECALCULATION_MAX_DAYS;
use holocron_protocol::operations::analytics::{
    AnalyticsHandlers, CorrectionState, Freshness, FreshnessArgs, Recalculation,
    RecalculationResult, RecalculationsStartArgs, RecalculationsStatusArgs,
};
use holocron_protocol::registry::Snapshot;
use holocron_storage::Lane;
use serde_json::json;

use super::policy::analytics_id;
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::maintenance::health;
use crate::state::AppState;

impl AnalyticsHandlers for AppState {
    async fn recalculations_start(
        &self,
        ctx: &mut CommandCtx,
        args: RecalculationsStartArgs,
    ) -> Result<RecalculationResult, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure, None)
            .await?;
        let mut errors = FieldErrors::default();
        let first = parse_day(&args.first_day, "first_day", &mut errors);
        let last = parse_day(&args.last_day, "last_day", &mut errors);
        errors.finish()?;
        let (first, last) = (first.expect("validated"), last.expect("validated"));
        let days = (last - first).num_days() + 1;
        if days < 1 {
            return Err(FieldErrors::one(
                "last_day",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        if days > RECALCULATION_MAX_DAYS {
            return Err(FieldErrors::one(
                "last_day",
                FieldErrorCode::TooMany,
                Some(RECALCULATION_MAX_DAYS as u32),
            ));
        }

        let site = scope.site().as_str();
        let job = jobs::recalculate(
            self.collector.storage(),
            analytics_id(site).map_err(ApiError::internal)?,
            first..=last,
        )
        .await
        .map_err(ApiError::internal)?;
        self.correction.notify_one();
        let id = RecalculationId::new(crate::db::new_id());
        let now = self.collector.clock().now_ms();
        let user = ctx.person()?;
        sqlx::query(
            "INSERT INTO recalculations (id, site_id, first_day, last_day, through_batch, requested_by, requested_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.as_str())
        .bind(site)
        .bind(&args.first_day)
        .bind(&args.last_day)
        .bind(job.through_batch)
        .bind(user.as_str())
        .bind(now)
        .execute(ctx.conn())
        .await?;
        ctx.record(
            &Topic::site(scope.site()),
            "site.recalculation_started",
            json!({ "id": id, "first_day": args.first_day, "last_day": args.last_day }),
        )
        .await?;
        Ok(RecalculationResult {
            recalculation: Recalculation {
                id,
                first_day: args.first_day,
                last_day: args.last_day,
                requested_at: now,
                state: state(job.days),
                pending_days: job.days as i64,
            },
        })
    }

    async fn recalculations_status(
        &self,
        ctx: &mut QueryCtx,
        args: RecalculationsStatusArgs,
    ) -> Result<Snapshot<RecalculationResult>, ApiError> {
        let scope = ctx
            .site_scope(&args.site, Capability::SiteConfigure)
            .await?;
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT first_day, last_day, through_batch, requested_at FROM recalculations
             WHERE id = ? AND site_id = ?",
        )
        .bind(args.recalculation.as_str())
        .bind(scope.site().as_str())
        .fetch_optional(ctx.conn())
        .await?;
        let (first_day, last_day, through, requested_at) = row.ok_or_else(ApiError::not_found)?;
        let day =
            |text: &str| NaiveDate::parse_from_str(text, DAY_FORMAT).map_err(ApiError::internal);
        let days = day(&first_day)?..=day(&last_day)?;
        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let pending = self
            .collector
            .storage()
            .read(Lane::Live, move |s| {
                jobs::pending_days(s.connection(), site, days, through)
            })
            .await?
            .value;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: RecalculationResult {
                recalculation: Recalculation {
                    id: args.recalculation,
                    first_day,
                    last_day,
                    requested_at,
                    state: state(pending),
                    pending_days: pending as i64,
                },
            },
        })
    }

    async fn analytics_freshness(
        &self,
        ctx: &mut QueryCtx,
        args: FreshnessArgs,
    ) -> Result<Snapshot<Freshness>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        let mut errors = FieldErrors::default();
        let in_range = |ms: i64| (MIN_TIME_MS..=MAX_TIME_MS).contains(&ms);
        if !in_range(args.start) {
            errors.add("start", FieldErrorCode::InvalidValue, None);
        }
        if !in_range(args.end) || args.end <= args.start {
            errors.add("end", FieldErrorCode::InvalidValue, None);
        }
        errors.finish()?;

        let site = analytics_id(scope.site().as_str()).map_err(ApiError::internal)?;
        let (start, end) = (args.start, args.end);
        let now = self.collector.clock().now_ms();
        let storage = self.collector.storage();
        let read = storage
            .read(Lane::Live, move |s| freshness(s, site, start, end))
            .await?;
        let mut data = wire_freshness(read.value, now);
        health::mark_behind(storage, read.watermark, now, &mut data)
            .await
            .map_err(ApiError::internal)?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data,
        })
    }
}

const DAY_FORMAT: &str = "%Y-%m-%d";

fn parse_day(text: &str, field: &str, errors: &mut FieldErrors) -> Option<NaiveDate> {
    let day = NaiveDate::parse_from_str(text, DAY_FORMAT).ok();
    if day.is_none() {
        errors.add(field, FieldErrorCode::InvalidFormat, None);
    }
    day
}

fn state(pending_days: u64) -> CorrectionState {
    match pending_days {
        0 => CorrectionState::Corrected,
        _ => CorrectionState::Pending,
    }
}
