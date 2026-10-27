//! `analytics.report`: report queries for agents (decision 0022).
//!
//! The order is budget, then authorization of every block's site, then the plan's checks, and
//! only then one analytical read that runs every block on the same snapshot.

use std::collections::HashMap;
use std::time::Instant;

use holocron_analytics::report_query::{self, Site};
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::limits::METRIC_VERSION;
use holocron_protocol::registry::Snapshot;
use holocron_protocol::report_query::{
    REPORT_QUERY_VERSION, ReportQuery, ReportQueryResult, ReportWatermark,
};
use holocron_storage::Lane;

use super::{hostnames, refused};
use crate::context::QueryCtx;
use crate::error::ApiError;
use crate::identity::Caller;
use crate::maintenance::health::STALE_AFTER;
use crate::sites::policy::analytics_id;
use crate::state::AppState;

impl AppState {
    pub(super) async fn report_query(
        &self,
        ctx: &mut QueryCtx,
        query: ReportQuery,
    ) -> Result<Snapshot<ReportQueryResult>, ApiError> {
        let started = Instant::now();
        let sub_queries = report_query::budget(&query).map_err(refused)?;
        let mut sites = HashMap::new();
        for block in &query.blocks {
            if sites.contains_key(&block.site) {
                continue;
            }
            let scope = ctx.site_scope(&block.site, Capability::SiteRead).await?;
            let hostnames = hostnames(ctx.conn(), &scope).await?;
            let retention = crate::retention::effective(
                &mut *ctx.conn(),
                crate::retention::Kind::Site,
                scope.site().as_str(),
            )
            .await?
            .ok_or_else(ApiError::not_found)?;
            let timezone: String = sqlx::query_scalar(
                "SELECT timezone FROM session_policies WHERE site_id = ? ORDER BY version DESC LIMIT 1",
            )
            .bind(scope.site().as_str())
            .fetch_one(ctx.conn())
            .await?;
            let site = Site {
                analytics: analytics_id(scope.site().as_str()).map_err(ApiError::internal)?,
                hostnames,
                retention,
                timezone,
            };
            sites.insert(block.site.clone(), site);
        }
        let topic = Topic::site(&query.blocks[0].site);
        let blocks = query.blocks.len();
        let plan = report_query::plan(query, &sites).map_err(refused)?;

        let now_ms = self.collector.clock().now_ms();
        let storage = self.collector.storage();
        let read = storage
            .read(Lane::Report, move |snapshot| {
                report_query::run(snapshot, &plan, &sites, now_ms)
            })
            .await?;
        let mut results = read.value.map_err(refused)?;
        let age = storage
            .inbox_age_after(read.watermark)
            .await
            .map_err(ApiError::internal)?;
        let watermark = ReportWatermark {
            batch: read.watermark.batch,
            sequence: read.watermark.sequence,
            at: now_ms - age.as_millis() as i64,
            stale: age > STALE_AFTER,
        };
        for freshness in results.iter_mut().filter_map(|b| b.freshness.as_mut()) {
            freshness.watermark_at = watermark.at;
            freshness.stale = watermark.stale;
        }

        let key = match ctx.caller() {
            Caller::Key(key) => Some(key.id.to_string()),
            _ => None,
        };
        tracing::info!(
            key = key.as_deref().unwrap_or("-"),
            blocks,
            sub_queries,
            duration_ms = started.elapsed().as_millis() as u64,
            "report query"
        );
        Ok(Snapshot {
            topic,
            data: ReportQueryResult {
                v: REPORT_QUERY_VERSION,
                watermark,
                metric_version: METRIC_VERSION,
                blocks: results,
            },
        })
    }
}
