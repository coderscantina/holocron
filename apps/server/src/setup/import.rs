//! The import phase of setup mode (decision 0021): the inspect preview, the run as a job inside
//! `serve` with its progress stream, the manifest, and discarding the import to start over.
//! The source URL and the Umami 2FA key live in the job's memory only; resuming after a restart
//! asks for them again, and the run's fingerprint refuses a different source.

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chrono_tz::Tz;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::retention::Retention;
use holocron_protocol::setup::{
    SetupDiscardArgs, SetupImportError, SetupImportOptions, SetupImportPreview,
    SetupImportProgress, SetupImportReport, SetupImportState, SetupImportStatus,
    SetupImportedLogin, SetupImportedSite, SetupInspectArgs, SetupInspection, SetupInventoryCount,
    SetupRetentionPreview, SetupRunArgs, SetupStepState, SetupTableRows,
};
use holocron_storage::Budgets;
use serde_json::{Value, json};

use super::{Phase, Shared};
use crate::db::Db;
use crate::error::{ApiError, FieldErrors};
use crate::http::operations::no_store;
use crate::imports::{self, Job, Options, Reporter};
use crate::init::{self, Exit, Failure};

/// Highest `chunk_rows`, as `holocron import run --chunk-rows` allows.
const MAX_CHUNK_ROWS: i64 = 100_000;

/// The import part of [`super::status`].
pub(super) async fn status(setup: &Shared, db: &Db) -> Result<SetupImportStatus, ApiError> {
    let progress = setup.progress.borrow().clone();
    let run = imports::stored_run(db).await?;
    let state = match &run {
        _ if progress.state == SetupImportState::Running => SetupImportState::Running,
        None => SetupImportState::None,
        Some(run) if run.manifest.is_some() => SetupImportState::Complete,
        Some(_) => SetupImportState::Interrupted,
    };
    Ok(SetupImportStatus {
        state,
        source: run.as_ref().map(|r| r.source.clone()),
        options: run
            .as_ref()
            .and_then(|r| serde_json::from_value::<Options>(r.options.clone()).ok())
            .map(|o| SetupImportOptions {
                timezone: o.timezone,
                retention: o.retention,
                login_domain: o.login_domain,
                chunk_rows: o.chunk_rows,
            }),
        error: progress.error,
        report: run.and_then(|r| r.manifest).map(|m| report(&m)),
    })
}

/// Inspects the source: the preview, or the refusal with its fix.
pub(super) async fn inspect(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupInspection>, ApiError> {
    let args: SetupInspectArgs = setup.admit(&headers, &body)?;
    setup.authorized(&headers, peer).await?;
    let timezone = timezone(&args.timezone)?;
    let inspected = imports::inspect(&args.source, timezone, setup.clock.now_ms()).await;
    let refusal = |failure: Failure| -> Result<Json<SetupInspection>, ApiError> {
        if failure.exit == Exit::Internal {
            return Err(ApiError::internal(failure.message));
        }
        Ok(Json(SetupInspection {
            preview: None,
            refusal: Some(error(&failure)),
        }))
    };
    match inspected {
        Ok((report, Ok(()))) => Ok(Json(SetupInspection {
            preview: Some(preview(&report)),
            refusal: None,
        })),
        Ok((_, Err(failure))) | Err(failure) => refusal(failure),
    }
}

/// Starts or resumes the import as a job of this process.
pub(super) async fn run(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupImportProgress>, ApiError> {
    let args: SetupRunArgs = setup.admit(&headers, &body)?;
    let (db, _) = setup.authorized(&headers, peer).await?;
    let mut errors = FieldErrors::default();
    let options = &args.options;
    if options.timezone.parse::<Tz>().is_err() {
        errors.add("timezone", FieldErrorCode::InvalidValue, None);
    }
    let domain = options.login_domain.trim().to_owned();
    if domain.is_empty() || domain.contains(['@', ' ']) {
        errors.add("login_domain", FieldErrorCode::InvalidFormat, None);
    }
    if !(1..=MAX_CHUNK_ROWS).contains(&options.chunk_rows) {
        errors.add("chunk_rows", FieldErrorCode::InvalidValue, None);
    }
    let key = args.two_factor_key.as_deref().map(str::trim);
    let two_factor_key = key
        .filter(|key| !key.is_empty())
        .map(imports::parse_two_factor_key);
    if two_factor_key.is_some_and(|key| key.is_none()) {
        errors.add("two_factor_key", FieldErrorCode::InvalidFormat, None);
    }
    errors.finish()?;

    // Under the phase lock, so no discard or finish runs while the job starts.
    let phase = setup.phase.lock().await;
    if !matches!(*phase, Phase::Import(_)) {
        return Err(ApiError::new(ErrorCode::SetupRequired));
    }
    let started = setup.progress.send_if_modified(|progress| {
        if progress.state == SetupImportState::Running {
            return false;
        }
        progress.state = SetupImportState::Running;
        progress.error = None;
        true
    });
    if !started {
        return Err(ApiError::new(ErrorCode::ImportInProgress));
    }
    let job = Job {
        data_dir: setup.config.data_dir.clone(),
        source: args.source,
        options: Options {
            timezone: args.options.timezone,
            login_domain: domain,
            chunk_rows: args.options.chunk_rows,
            retention: args.options.retention,
        },
        two_factor_key: two_factor_key.flatten(),
    };
    // The same budgets as `holocron import run`: the import keeps its memory bound by chunks.
    let budgets = Budgets {
        disk_reserve_bytes: setup.config.disk_reserve_bytes,
        ..Budgets::default()
    };
    let now = setup.clock.now_ms();
    let reporter = Reporter::new(setup.progress.clone());
    tracing::info!("setup: import started");
    let job_setup = setup.clone();
    tokio::spawn(async move {
        let result = imports::import(job, budgets, now, reporter).await;
        let stopped = match &result {
            Ok(_) => {
                tracing::info!("setup: import complete");
                None
            }
            Err(failure) => {
                tracing::warn!(code = %failure.code, message = %failure.message, "setup: import stopped");
                Some(error(failure))
            }
        };
        let started = matches!(
            imports::state(&db).await,
            Ok(imports::ImportState::Running { .. })
        );
        job_setup.progress.send_modify(|progress| {
            progress.state = match (&stopped, started) {
                (None, _) => SetupImportState::Complete,
                (Some(_), true) => SetupImportState::Interrupted,
                (Some(_), false) => SetupImportState::None,
            };
            if stopped.is_none() {
                for step in &mut progress.steps {
                    step.state = SetupStepState::Done;
                    step.eta_seconds = None;
                }
            }
            progress.error = stopped;
        });
    });
    drop(phase);
    Ok(Json(setup.progress.borrow().clone()))
}

/// The progress stream: the current frame, then one per change, until setup mode stops.
pub(super) async fn progress(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    setup.authorized(&headers, peer).await?;
    let frames = setup.progress.subscribe();
    let closing = setup.closing.subscribe();
    let stream = futures_util::stream::unfold(
        (frames, closing, true),
        |(mut frames, mut closing, first)| async move {
            if !first {
                tokio::select! {
                    changed = frames.changed() => changed.ok()?,
                    _ = closing.wait_for(|closing| *closing) => return None,
                }
            }
            let frame = frames.borrow_and_update().clone();
            let event = Event::default()
                .json_data(&frame)
                .expect("frames serialize");
            Some((Ok::<_, Infallible>(event), (frames, closing, false)))
        },
    );
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// The whole manifest of a complete import, as a download.
pub(super) async fn manifest(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (db, _) = setup.authorized(&headers, peer).await?;
    let manifest = imports::stored_run(&db)
        .await?
        .and_then(|run| run.manifest)
        .ok_or_else(ApiError::not_found)?;
    let mut response = no_store(Json(manifest).into_response());
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"holocron-import-manifest.json\""),
    );
    Ok(response)
}

/// Removes the imported data and keeps the admin and `secrets.key`, after the instance name.
pub(super) async fn discard(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let args: SetupDiscardArgs = setup.admit(&headers, &body)?;
    let (_, user) = setup.authorized(&headers, peer).await?;
    if args.confirm.trim() != setup.config.instance_name.trim() {
        return Err(FieldErrors::one(
            "confirm",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    let mut phase = setup.phase.lock().await;
    let Phase::Import(installed) = &mut *phase else {
        return Err(ApiError::new(ErrorCode::SetupRequired));
    };
    if setup.progress.borrow().state == SetupImportState::Running {
        return Err(ApiError::new(ErrorCode::ImportInProgress));
    }
    let dir = &setup.config.data_dir;
    installed.db.close().await;
    let reset = init::reset(dir, &user).await;
    // Reopened either way: after a failed reset the directory reads as an interrupted init.
    installed.db = Db::connect(dir).await.map_err(ApiError::internal)?;
    reset.map_err(super::failed)?;
    setup.progress.send_replace(SetupImportProgress::default());
    tracing::warn!("setup: the import was discarded; the admin and secrets.key stay");
    Ok(no_store(Json(json!({})).into_response()))
}

fn timezone(name: &str) -> Result<Tz, ApiError> {
    name.parse()
        .map_err(|_| FieldErrors::one("timezone", FieldErrorCode::InvalidValue, None))
}

fn error(failure: &Failure) -> SetupImportError {
    SetupImportError {
        code: failure.code.to_string(),
        message: failure.message.clone(),
    }
}

/// The preview of an accepted inspection report.
fn preview(report: &Value) -> SetupImportPreview {
    let preview = &report["preview"];
    let events = &preview["events"];
    let int = |value: &Value| value.as_i64().unwrap_or(0);
    SetupImportPreview {
        source: report["source"].as_str().unwrap_or_default().to_owned(),
        tables: preview["rows"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(table, rows)| SetupTableRows {
                table: table.clone(),
                rows: int(rows),
            })
            .collect(),
        events: int(&events["total"]),
        first_event: events["first"].as_i64(),
        last_event: events["last"].as_i64(),
        retention: Retention::ALL
            .into_iter()
            .map(|retention| SetupRetentionPreview {
                retention,
                events_outside: int(&events["outside_retention"][retention.code()]),
                cutoff: events["retention_cutoffs"][retention.code()].as_i64(),
            })
            .collect(),
        source_bytes: int(&preview["source_bytes"]),
        warnings: serde_json::from_value(report["warnings"].clone()).unwrap_or_default(),
    }
}

/// The parts of the manifest the report step shows.
fn report(manifest: &Value) -> SetupImportReport {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let list = |value: &Value| value.as_array().cloned().unwrap_or_default();
    SetupImportReport {
        reconciled: manifest["status"] == "reconciled",
        differences: list(&manifest["differences"]).iter().map(text).collect(),
        sites: list(&manifest["sites"])
            .iter()
            .map(|site| SetupImportedSite {
                id: text(&site["id"]),
                kind: text(&site["kind"]),
                name: text(&site["name"]),
                events: site["events"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter_map(|(_, kind)| kind["destination"].as_i64())
                    .sum(),
            })
            .collect(),
        logins: list(&manifest["logins"])
            .iter()
            .map(|login| SetupImportedLogin {
                username: text(&login["username"]),
                email: text(&login["email"]),
                password: text(&login["password"]),
                two_factor: text(&login["two_factor"]),
            })
            .collect(),
        inventory: list(&manifest["inventory"])
            .iter()
            .map(|entry| SetupInventoryCount {
                table: text(&entry["table"]),
                reason: text(&entry["reason"]),
                count: entry["count"].as_i64().unwrap_or(0),
            })
            .collect(),
        events_not_imported: manifest["events_not_imported"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(reason, count)| {
                let count = count.as_i64().filter(|count| *count > 0)?;
                Some(SetupInventoryCount {
                    table: String::new(),
                    reason: reason.clone(),
                    count,
                })
            })
            .collect(),
    }
}
