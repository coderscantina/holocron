//! `operator.status`: what an install admin needs to see whether the server is healthy and
//! whether the disk holds the retained data. Built from counts, sizes, times, states and fixed
//! names only; nothing from an event, an identity input or a site.

use holocron_protocol::access::InstallRole;
use holocron_protocol::envelope::{Empty, Topic};
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::operations::operator::{
    MaintenanceOutcome, MaintenanceState, MaintenanceTask, OperatorHandlers, OperatorStatus,
};
use holocron_protocol::operations::recordings::RecordingBudget;
use holocron_protocol::registry::Snapshot;
use sqlx::SqlitePool;

use super::{health, samples};
use crate::context::QueryCtx;
use crate::error::ApiError;
use crate::identity;
use crate::state::AppState;

impl OperatorHandlers for AppState {
    async fn operator_status(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<OperatorStatus>, ApiError> {
        let user = ctx.person()?;
        let account = identity::account(ctx.conn(), &user).await?;
        if account.is_none_or(|a| a.install_role != InstallRole::InstallAdmin) {
            return Err(ApiError::new(ErrorCode::Forbidden));
        }

        let collection = crate::collect::status::snapshot(self).await?;
        let r = &self.recordings;
        let usage = r.store.usage().await?;
        let recording = RecordingBudget {
            used_bytes: usage.bytes,
            cap_bytes: r.cap_bytes() as i64,
            saved_bytes: usage.saved_bytes,
            saved_cap_bytes: r.saved_cap_bytes() as i64,
            captures: usage.captures,
            file_bytes: usage.file_bytes,
            refusing: r.refusing(self.collector.clock().now_ms()),
        };
        let pool = &self.db.reader;
        let mut maintenance = Vec::with_capacity(3);
        maintenance.extend(latest_job(pool, MaintenanceTask::Retention, "retention").await?);
        maintenance.extend(
            self.maintenance
                .last_compaction()
                .map(|pass| MaintenanceOutcome {
                    task: MaintenanceTask::Compaction,
                    state: if pass.failed {
                        MaintenanceState::Failed
                    } else {
                        MaintenanceState::Done
                    },
                    started_at: Some(pass.started_at),
                    finished_at: Some(pass.finished_at),
                    error_code: None,
                }),
        );
        maintenance.extend(latest_job(pool, MaintenanceTask::Backup, "backup").await?);
        let kept = samples::list(pool).await?;
        let forecast = kept.last().map(|latest| {
            samples::forecast(latest, recording.cap_bytes, collection.disk_reserve_bytes)
        });
        Ok(Snapshot {
            topic: Topic::installation(),
            data: OperatorStatus {
                problems: health::check(self).await.problems,
                disk: samples::disk(self).map_err(ApiError::internal)?,
                samples: kept.iter().map(samples::Sample::summary).collect(),
                collection,
                recording,
                maintenance,
                forecast,
            },
        })
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    state: String,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    result: Option<String>,
}

/// The newest job of `kind` with its state and, for a failure, its fixed code.
async fn latest_job(
    pool: &SqlitePool,
    task: MaintenanceTask,
    kind: &str,
) -> Result<Option<MaintenanceOutcome>, ApiError> {
    let row: Option<JobRow> = sqlx::query_as(
        "SELECT state, started_at, finished_at, result FROM maintenance_jobs WHERE kind = ?
         ORDER BY created_at DESC, rowid DESC LIMIT 1",
    )
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    let Some(JobRow {
        state,
        started_at,
        finished_at,
        result,
    }) = row
    else {
        return Ok(None);
    };
    let state = match state.as_str() {
        "queued" => MaintenanceState::Queued,
        "running" => MaintenanceState::Running,
        "done" => MaintenanceState::Done,
        _ => MaintenanceState::Failed,
    };
    let error_code = (state == MaintenanceState::Failed)
        .then(|| {
            let result: serde_json::Value = serde_json::from_str(result.as_deref()?).ok()?;
            Some(result["code"].as_str()?.to_owned())
        })
        .flatten();
    Ok(Some(MaintenanceOutcome {
        task,
        state,
        started_at,
        finished_at,
        error_code,
    }))
}
