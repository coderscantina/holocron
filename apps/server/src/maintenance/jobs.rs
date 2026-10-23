//! The job queue in `maintenance_jobs` and the runner that works through it, one job at a
//! time. Queued jobs run first, oldest first; an erasure waiting for transfer is retried after
//! them. Retention queues itself hourly (0006: at least hourly). The runner also takes the
//! storage sample at start and then daily.

use std::path::PathBuf;
use std::time::Duration;

use holocron_storage::Erasure;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Executor, Sqlite, SqlitePool};
use tokio::sync::watch;
use uuid::Uuid;

use super::{backup, diagnostics, erasure, internal, retention, samples};
use crate::init::{Exit, Failure};
use crate::state::AppState;

const POLL: Duration = Duration::from_secs(1);
const RETENTION_EVERY_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Job {
    /// Every record of a deleted site, admitted before or after the deletion.
    EraseSite {
        site: Uuid,
    },
    /// A site's records admitted up to `through_seq`, the last inbox sequence when the reset
    /// was asked for. The site and its settings stay.
    ResetSite {
        site: Uuid,
        through_seq: i64,
    },
    /// One persistent profile's records. `profile` is the keyed profile ID, never the
    /// identify ID it was derived from.
    EraseProfile {
        site: Uuid,
        profile: Uuid,
    },
    Retention,
    Backup {
        to: PathBuf,
    },
    Diagnostics,
}

impl Job {
    fn kind(&self) -> &'static str {
        match self {
            Job::EraseSite { .. } => "erase_site",
            Job::ResetSite { .. } => "reset_site",
            Job::EraseProfile { .. } => "erase_profile",
            Job::Retention => "retention",
            Job::Backup { .. } => "backup",
            Job::Diagnostics => "diagnostics",
        }
    }
}

/// What running a job came to.
pub enum Outcome {
    Done(Value),
    /// Not finished yet (an erasure waiting for transfer); the job runs again later.
    Retry,
    Failed(Failure),
}

/// Queues `job`, in the caller's transaction when it passes one.
pub async fn enqueue<'e>(
    conn: impl Executor<'e, Database = Sqlite>,
    job: &Job,
    now_ms: i64,
) -> sqlx::Result<String> {
    let id = crate::db::new_id();
    sqlx::query(
        "INSERT INTO maintenance_jobs (id, kind, args, state, created_at) VALUES (?, ?, ?, 'queued', ?)",
    )
    .bind(&id)
    .bind(job.kind())
    .bind(serde_json::to_string(job).expect("jobs serialize"))
    .bind(now_ms)
    .execute(conn)
    .await?;
    Ok(id)
}

/// A finished job's result: its value, or the failure it reported.
pub async fn finished(pool: &SqlitePool, id: &str) -> sqlx::Result<Option<Result<Value, Failure>>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT state, result FROM maintenance_jobs WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let Some((state, result)) = row else {
        return Ok(None);
    };
    let result: Value = result
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| sqlx::Error::Decode(e.into()))?
        .unwrap_or(Value::Null);
    Ok(match state.as_str() {
        "done" => Some(Ok(result)),
        "failed" => Some(Err(failure_of(&result))),
        _ => None,
    })
}

fn failure_json(failure: &Failure) -> Value {
    json!({ "exit": failure.exit, "code": failure.code, "message": failure.message })
}

fn failure_of(result: &Value) -> Failure {
    let exit = serde_json::from_value(result["exit"].clone()).unwrap_or(Exit::Internal);
    Failure::new(
        exit,
        result["code"].as_str().unwrap_or("internal").to_owned(),
        result["message"].as_str().unwrap_or_default().to_owned(),
    )
}

/// Runs queued jobs until `stop` changes. Started with the server.
pub async fn follow(state: AppState, mut stop: watch::Receiver<bool>) {
    if let Err(error) = abandon_interrupted(&state).await {
        tracing::error!(error = %error, "maintenance jobs could not be recovered");
    }
    let mut next_sample = 0;
    while !*stop.borrow() {
        if let Err(error) = schedule_retention(&state).await {
            tracing::error!(error = %error, "retention could not be scheduled");
        }
        let now = state.collector.clock().now_ms();
        if now >= next_sample {
            next_sample = match samples::take(&state).await {
                Ok(_) => now + samples::DAY_MS,
                Err(error) => {
                    tracing::error!(error = %error, "storage sample failed; retrying in an hour");
                    now + samples::RETRY_MS
                }
            };
        }
        loop {
            match run_next(&state).await {
                Ok(Some(Outcome::Done(_) | Outcome::Failed(_))) if !*stop.borrow() => continue,
                Ok(_) => break,
                Err(error) => {
                    tracing::error!(error = %error, "maintenance job queue failed; retrying");
                    break;
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = state.maintenance.wake.notified() => {}
            _ = stop.changed() => {}
        }
    }
}

/// Runs the queue until job `id` finished, for operator commands without a server.
pub async fn run_until(state: &AppState, id: &str) -> Result<Value, Failure> {
    abandon_interrupted(state).await.map_err(internal)?;
    loop {
        if let Some(result) = finished(&state.db.reader, id).await.map_err(internal)? {
            return result;
        }
        if let Some(Outcome::Retry) | None = run_next(state).await.map_err(internal)? {
            tokio::time::sleep(POLL).await;
        }
    }
}

/// Backups and diagnostics that a stop interrupted are failed: the operator asks again.
/// Erasure and retention jobs run again from the start; each of their steps checks what is
/// already done.
async fn abandon_interrupted(state: &AppState) -> sqlx::Result<()> {
    let interrupted = failure_json(&Failure::new(
        Exit::Internal,
        "interrupted",
        "the server stopped while this job ran; run the command again",
    ));
    sqlx::query(
        "UPDATE maintenance_jobs SET state = 'failed', result = ?, finished_at = ?
         WHERE state = 'running' AND kind IN ('backup', 'diagnostics')",
    )
    .bind(interrupted.to_string())
    .bind(state.collector.clock().now_ms())
    .execute(&state.db.writer)
    .await?;
    Ok(())
}

async fn schedule_retention(state: &AppState) -> sqlx::Result<()> {
    let now = state.collector.clock().now_ms();
    let due: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM maintenance_jobs WHERE kind = 'retention'
             AND (state IN ('queued', 'running') OR created_at > ?))",
    )
    .bind(now - RETENTION_EVERY_MS)
    .fetch_one(&state.db.reader)
    .await?;
    if due {
        enqueue(&state.db.writer, &Job::Retention, now).await?;
    }
    Ok(())
}

/// Claims and runs the next job. `None` when the queue is empty.
async fn run_next(state: &AppState) -> sqlx::Result<Option<Outcome>> {
    let now = state.collector.clock().now_ms();
    let claimed: Option<(String, String)> = sqlx::query_as(
        "UPDATE maintenance_jobs SET state = 'running', started_at = coalesce(started_at, ?)
         WHERE id = (SELECT id FROM maintenance_jobs WHERE state IN ('queued', 'running')
                     ORDER BY state = 'running', created_at, rowid LIMIT 1)
         RETURNING id, args",
    )
    .bind(now)
    .fetch_optional(&state.db.writer)
    .await?;
    let Some((id, args)) = claimed else {
        return Ok(None);
    };
    let outcome = match serde_json::from_str::<Job>(&args) {
        Ok(job) => execute(state, &id, &job).await,
        Err(error) => Outcome::Failed(internal(format!("unreadable job: {error}"))),
    };
    let finish = |state: &str, result: Value| {
        sqlx::query(
            "UPDATE maintenance_jobs SET state = ?, result = ?, finished_at = ? WHERE id = ?",
        )
        .bind(state.to_owned())
        .bind(result.to_string())
        .bind(now)
        .bind(id.clone())
    };
    match &outcome {
        Outcome::Done(result) => {
            finish("done", result.clone())
                .execute(&state.db.writer)
                .await?;
        }
        Outcome::Failed(failure) => {
            tracing::error!(job = %id, code = %failure.code, "maintenance job failed");
            finish("failed", failure_json(failure))
                .execute(&state.db.writer)
                .await?;
        }
        Outcome::Retry => {}
    }
    Ok(Some(outcome))
}

async fn execute(state: &AppState, id: &str, job: &Job) -> Outcome {
    let (erasure, through_seq) = match *job {
        Job::EraseSite { site } => (Erasure::Site(site.as_u128()), None),
        Job::ResetSite { site, through_seq } => {
            (Erasure::SiteHistory(site.as_u128()), Some(through_seq))
        }
        Job::EraseProfile { site, profile } => (
            Erasure::Profile {
                site: site.as_u128(),
                profile: profile.as_u128(),
            },
            None,
        ),
        Job::Retention => return outcome(retention::run(state).await),
        Job::Backup { ref to } => {
            return outcome(
                backup::take(backup::Source::Running(state), to)
                    .await
                    .map(|(path, manifest)| backup::summary(&path, &manifest)),
            );
        }
        Job::Diagnostics => return outcome(diagnostics::collect(state).await),
    };
    // An erasure never gives up: deleted data must go even when a step fails for a while.
    match erasure::run(state, id, erasure, through_seq).await {
        Ok(Some(result)) => Outcome::Done(result),
        Ok(None) => Outcome::Retry,
        Err(failure) => {
            tracing::error!(job = %id, code = %failure.code, message = %failure.message, "erasure step failed; retrying");
            Outcome::Retry
        }
    }
}

fn outcome(result: Result<Value, Failure>) -> Outcome {
    match result {
        Ok(value) => Outcome::Done(value),
        Err(failure) => Outcome::Failed(failure),
    }
}
