//! Setup mode (decision 0021): `serve` on a directory that is not set up answers only the setup
//! wizard, then returns so `serve` starts normally on the same listener.
//!
//! On an empty directory nothing touches the disk before the admin step, which runs `holocron
//! init`'s code with the chosen password and signs the admin in. Until then the setup token
//! authorizes. The fresh path ends setup right there. The import path stays in setup mode while
//! the Umami import runs inside it ([`import`]), authorized by the admin's session, and ends
//! when the admin acknowledges the report. A directory with an unfinished import, after a
//! restart or a killed `holocron import run`, starts in that phase again.
//!
//! The restore path ([`restore`]) stays in the claim phase, authorized by the token: it restores
//! a backup into the empty directory with `holocron restore`'s code, upgrades it with
//! `holocron upgrade`'s code when the backup is older, and ends setup like the fresh path.
//! A `restore_incomplete` directory starts here too and the wizard offers the step again.

mod import;
mod restore;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use holocron_protocol::config::RuntimeConfig;
use holocron_protocol::errors::ErrorCode;
use holocron_protocol::ids::UserId;
use holocron_protocol::operations::auth::{Account, LoginResult};
use holocron_protocol::setup::{
    SetupAdminArgs, SetupAdminPath, SetupCheck, SetupImportProgress, SetupImportState, SetupPhase,
    SetupRestoreProgress, SetupSignInArgs, SetupStatus, SetupTokenArgs,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::sync::watch;

use crate::clock::Clock;
use crate::config::Config;
use crate::context::SessionChange;
use crate::db::{Db, now_ms};
use crate::error::{ApiError, FieldErrors};
use crate::http::operations::{check_origin, no_store, session_cookies};
use crate::identity::{self, RateLimiter, token_digest};
use crate::init::{self, Exit, Failure, InitArgs};
use crate::maintenance::lock::DataLock;

/// Wrong tokens allowed per minute, as many as sign-in attempts.
const ATTEMPTS_PER_MINUTE: u32 = 10;
const TOKEN_GROUPS: usize = 4;
const TOKEN_GROUP_CHARS: usize = 4;

/// How a setup run ended.
pub enum Outcome {
    /// Setup finished; the directory is initialized and `serve` continues.
    Initialized,
    /// The stop signal came first.
    Stopped,
}

struct Setup {
    config: Config,
    clock: Clock,
    /// Digest of the normalized token.
    token: String,
    /// An admin exists, so the token is spent.
    claimed: AtomicBool,
    failures: Mutex<Option<(Instant, u32)>>,
    sign_ins: RateLimiter,
    /// One phase change at a time: the admin step, a discard, the finish.
    phase: tokio::sync::Mutex<Phase>,
    progress: watch::Sender<SetupImportProgress>,
    restore: watch::Sender<SetupRestoreProgress>,
    done: watch::Sender<bool>,
    /// Set when setup mode stops serving, so open progress streams end.
    closing: watch::Sender<bool>,
}

enum Phase {
    /// No admin yet.
    Claim,
    /// A restore runs in the data directory.
    Restoring,
    /// The admin exists and an import waits for the wizard.
    Import(Installed),
    /// Setup finished; `serve` takes over.
    Done,
}

/// The initialized directory of the import phase: its lock, held until setup ends, and its
/// control database.
struct Installed {
    db: Db,
    _lock: DataLock,
}

impl Installed {
    async fn open(dir: &Path) -> Result<Self, Failure> {
        let lock = DataLock::acquire(dir)?;
        crate::maintenance::versions::check_dir(dir).await?;
        let db = Db::connect(dir)
            .await
            .map_err(|e| Failure::storage(dir, e))?;
        Ok(Self { db, _lock: lock })
    }
}

type Shared = Arc<Setup>;

/// Whether `serve` has to set up `dir` first: it is missing, empty, an init or a restore stopped
/// halfway, or the wizard's import path has not finished.
pub fn needed(dir: &Path) -> Result<bool, Failure> {
    match init::check_initialized(dir) {
        Ok(()) => Ok(dir.join(init::SETUP_IMPORT_MARKER).exists()),
        Err(failure)
            if matches!(
                &*failure.code,
                "not_initialized" | "init_incomplete" | "restore_incomplete"
            ) =>
        {
            let empty = match std::fs::read_dir(dir) {
                // The lock file is whoever held the directory last, not data.
                Ok(mut entries) => entries.all(|entry| {
                    entry
                        .is_ok_and(|entry| entry.file_name() == crate::maintenance::lock::LOCK_FILE)
                }),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                Err(e) => return Err(Failure::storage(dir, e)),
            };
            if empty || matches!(&*failure.code, "init_incomplete" | "restore_incomplete") {
                Ok(true)
            } else {
                Err(failure)
            }
        }
        Err(failure) => Err(failure),
    }
}

/// [`needed`], or an import into `dir` was started and has not finished, for example a killed
/// `holocron import run`. The wizard resumes it (decision 0021).
pub async fn required(dir: &Path) -> Result<bool, Failure> {
    if needed(dir)? {
        return Ok(true);
    }
    crate::maintenance::versions::check_dir(dir).await?;
    let db = Db::connect(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    let state = crate::imports::state(&db).await;
    db.close().await;
    Ok(matches!(
        state.map_err(|e| Failure::storage(dir, e))?,
        crate::imports::ImportState::Running { .. }
    ))
}

/// Serves the wizard on `listener` until setup finishes or `stopped` fires. `clock` pins the
/// clock of an import it starts.
pub async fn run(
    config: Config,
    clock: Clock,
    listener: tokio::net::TcpListener,
    mut stopped: watch::Receiver<bool>,
) -> Result<Outcome, Failure> {
    let dir = config.data_dir.clone();
    let phase = match init::check_initialized(&dir) {
        Ok(()) => Phase::Import(Installed::open(&dir).await?),
        Err(_) => Phase::Claim,
    };
    let claimed = matches!(phase, Phase::Import(_));
    let token = config.setup_token.clone().unwrap_or_else(generate_token);
    if normalize(&token).is_empty() {
        return Err(Failure::new(
            Exit::Usage,
            "usage",
            "HOLOCRON_SETUP_TOKEN must hold letters or digits",
        ));
    }
    let progress = match &phase {
        Phase::Import(installed) => crate::imports::stored_progress(&installed.db)
            .await
            .map_err(|e| Failure::storage(&dir, e))?,
        _ => SetupImportProgress::default(),
    };
    if claimed {
        tracing::warn!(
            dir = %dir.display(),
            "setup mode: an Umami import into this data directory has not finished; open the dashboard and sign in as the install admin to continue it"
        );
    } else {
        if dir
            .join(crate::maintenance::restore::IN_PROGRESS_MARKER)
            .exists()
        {
            tracing::warn!(
                dir = %dir.display(),
                "setup mode: a restore into this data directory stopped partway; open the dashboard to restore the backup again"
            );
        }
        if config.setup_token.is_none() {
            // The one place the token appears. The operator copies it from here into the wizard.
            tracing::warn!("setup token: {token}");
        }
        tracing::warn!(
            dir = %dir.display(),
            "setup mode: the data directory is empty; open the dashboard to set up Holocron"
        );
    }

    let (done, mut finished) = watch::channel(false);
    let setup = Arc::new(Setup {
        token: token_digest(&normalize(&token)),
        config,
        clock,
        claimed: AtomicBool::new(claimed),
        failures: Mutex::new(None),
        sign_ins: RateLimiter::per_minute(ATTEMPTS_PER_MINUTE),
        phase: tokio::sync::Mutex::new(phase),
        progress: watch::channel(progress).0,
        restore: watch::channel(SetupRestoreProgress::default()).0,
        done,
        closing: watch::channel(false).0,
    });
    let app = router(setup.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let mut stop_reason = stopped.clone();
    let closing = setup.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = finished.wait_for(|done| *done) => {}
                _ = stop_reason.wait_for(|stop| *stop) => {}
            }
            closing.closing.send_replace(true);
        })
        .await
        .map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string()))?;

    if *setup.done.borrow() {
        tracing::info!("setup finished; starting");
        Ok(Outcome::Initialized)
    } else {
        // A running import stops with the process; its checkpoints let the wizard resume it.
        if let Phase::Import(installed) = &*setup.phase.lock().await {
            installed.db.close().await;
        }
        let _ = stopped.wait_for(|stop| *stop).await;
        Ok(Outcome::Stopped)
    }
}

fn router(setup: Shared) -> Router {
    let web_dir = setup.config.web_dir.clone();
    let api = Router::new()
        .route("/setup/status", get(status))
        .route("/setup/check", post(check))
        .route("/setup/admin", post(admin))
        .route("/setup/sign-in", post(sign_in))
        .route("/setup/finish", post(finish))
        .route("/setup/import/inspect", post(import::inspect))
        .route("/setup/import/run", post(import::run))
        .route("/setup/import/progress", get(import::progress))
        .route("/setup/import/manifest", get(import::manifest))
        .route("/setup/import/discard", post(import::discard))
        .route("/setup/restore/backups", post(restore::backups))
        .route("/setup/restore/inspect", post(restore::inspect))
        .route("/setup/restore/start", post(restore::start))
        .route("/setup/restore/progress", get(restore::progress))
        .fallback(setup_required);
    let routes = Router::new()
        .route("/health", get(health))
        .route("/config.json", get(runtime_config))
        .nest("/api", api)
        .with_state(setup);
    crate::http::web::files(routes, web_dir.as_deref())
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION"), "setup": true }))
}

async fn runtime_config(State(setup): State<Shared>) -> impl IntoResponse {
    no_store(
        Json(RuntimeConfig {
            api_base: "/api".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            instance_name: setup.config.instance_name.clone(),
            dbip_attribution: false,
            setup: true,
        })
        .into_response(),
    )
}

/// Collection and every dashboard operation wait for setup. The body is read first: closing
/// with it unread can reset the connection, and the tracker would see that instead of a 503.
async fn setup_required(_body: Bytes) -> ApiError {
    ApiError::new(ErrorCode::SetupRequired)
}

/// Where setup stands, and for the signed-in admin how far the import is.
async fn status(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let db = match &*setup.phase.lock().await {
        Phase::Claim | Phase::Restoring => None,
        Phase::Import(installed) => Some(installed.db.clone()),
        Phase::Done => return Err(ApiError::new(ErrorCode::SetupRequired)),
    };
    let mut status = SetupStatus {
        phase: SetupPhase::Claim,
        restore_incomplete: db.is_none()
            && setup
                .config
                .data_dir
                .join(crate::maintenance::restore::IN_PROGRESS_MARKER)
                .exists(),
        account: None,
        import: None,
    };
    if let Some(db) = db {
        status.phase = SetupPhase::Import;
        if let Some(user) = admin_session(&db, &headers, peer).await? {
            let mut conn = db.reader.acquire().await?;
            status.account = identity::account(&mut conn, &user).await?;
            drop(conn);
            status.import = Some(import::status(&setup, &db).await?);
        }
    }
    Ok(no_store(Json(status).into_response()))
}

async fn check(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SetupCheck>, ApiError> {
    let args: SetupTokenArgs = setup.admit(&headers, &body)?;
    setup.verify(&args.token)?;
    let config = &setup.config;
    let address = crate::collect::client::visitor_ip(peer.ip(), &headers, &config.collect);
    let disk = existing_ancestor(&config.data_dir);
    let disk_free_bytes = holocron_storage::free_bytes(disk).map_err(ApiError::internal)?;
    Ok(Json(SetupCheck {
        version: env!("CARGO_PKG_VERSION").into(),
        public_origin: config.public_origin.clone(),
        visitor_address: address.to_string(),
        visitor_address_private: is_private(address),
        disk_free_bytes,
        disk_reserve_bytes: config.disk_reserve_bytes,
    }))
}

async fn admin(
    State(setup): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let args: SetupAdminArgs = setup.admit(&headers, &body)?;
    setup.verify(&args.token)?;
    let email = identity::normalize_email(&args.email);
    let mut errors = FieldErrors::default();
    if let Some((code, limit)) = identity::email_error(&email) {
        errors.add("email", code, limit);
    }
    errors.finish()?;
    identity::validate_password("password", &args.password)?;

    let mut phase = setup.phase.lock().await;
    if !matches!(*phase, Phase::Claim) {
        return Err(ApiError::new(ErrorCode::SetupRequired));
    }
    let dir = setup.config.data_dir.clone();
    let path = args.path.unwrap_or_default();
    let init_args = InitArgs {
        data_dir: dir.clone(),
        admin_email: email,
        admin_password: Some(args.password),
    };
    let initialized = match path {
        SetupAdminPath::Fresh => init::init(init_args).await,
        SetupAdminPath::Import => init::init_for_import(init_args).await,
    }
    .map_err(failed)?;
    setup.claimed.store(true, Ordering::SeqCst);

    let (db, installed) = match path {
        SetupAdminPath::Fresh => (Db::open(&dir).await.map_err(ApiError::internal)?, None),
        SetupAdminPath::Import => {
            let installed = Installed::open(&dir).await.map_err(failed)?;
            (installed.db.clone(), Some(installed))
        }
    };
    let signed_in = async {
        let mut conn = db.writer.acquire().await?;
        let token = identity::insert_session(&mut conn, &initialized.admin, now_ms()).await?;
        let account = identity::account(&mut conn, &initialized.admin).await?;
        Ok::<_, sqlx::Error>((token, account))
    }
    .await;
    if installed.is_none() {
        db.close().await;
    }
    let (token, account) = signed_in?;
    let account = account.ok_or_else(|| ApiError::internal("the new admin has no account"))?;
    tracing::info!(email = %account.email, ?path, "setup: install admin created");

    let response = signed_in_response(&setup.config, token, account);
    match installed {
        Some(installed) => *phase = Phase::Import(installed),
        None => {
            *phase = Phase::Done;
            // Graceful shutdown lets this response finish before `serve` takes over.
            let _ = setup.done.send(true);
        }
    }
    Ok(response)
}

/// Signs the install admin in again in the import phase, for example after a restart.
async fn sign_in(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let args: SetupSignInArgs = setup.admit(&headers, &body)?;
    let db = setup.installed().await?;
    setup.sign_ins.check(&peer.ip().to_string())?;
    let mut conn = db.writer.acquire().await?;
    let (token, account) =
        identity::setup_sign_in(&mut conn, &args.email, args.password, now_ms()).await?;
    tracing::info!(email = %account.email, "setup: install admin signed in");
    Ok(signed_in_response(&setup.config, token, account))
}

/// Acknowledges the import report, or continues without an import when none was started (a
/// discard removes one first): setup ends and `serve` starts normally.
async fn finish(
    State(setup): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let _: serde_json::Value = setup.admit(&headers, &body)?;
    let (db, _) = setup.authorized(&headers, peer).await?;
    let mut phase = setup.phase.lock().await;
    let settled = crate::imports::stored_run(&db)
        .await?
        .is_none_or(|run| run.manifest.is_some());
    let running = setup.progress.borrow().state == SetupImportState::Running;
    if !settled || running || !matches!(*phase, Phase::Import(_)) {
        return Err(ApiError::new(ErrorCode::ImportInProgress));
    }
    let dir = &setup.config.data_dir;
    std::fs::remove_file(dir.join(init::SETUP_IMPORT_MARKER))
        .or_else(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
        .map_err(ApiError::internal)?;
    if let Phase::Import(installed) = std::mem::replace(&mut *phase, Phase::Done) {
        installed.db.close().await;
    }
    tracing::info!("setup: finished");
    let _ = setup.done.send(true);
    Ok(no_store(Json(json!({})).into_response()))
}

/// `auth.login`'s `signed_in` answer with the session cookie.
fn signed_in_response(config: &Config, token: String, account: Account) -> Response {
    let mut response = no_store(Json(LoginResult::SignedIn { account }).into_response());
    for cookie in session_cookies(config, &SessionChange::Start(token)) {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// An init failure as the wizard sees it.
fn failed(failure: Failure) -> ApiError {
    ApiError::internal(format!("{}: {}", failure.code, failure.message))
}

/// The install admin a session cookie belongs to, `None` without a live session, `forbidden`
/// for anyone else.
async fn admin_session(
    db: &Db,
    headers: &HeaderMap,
    peer: SocketAddr,
) -> Result<Option<UserId>, ApiError> {
    let caller = identity::resolve_caller(db, headers, peer.ip().to_string()).await?;
    if caller.user().is_none() {
        return Ok(None);
    }
    let mut conn = db.reader.acquire().await?;
    crate::sites::scope::require_install_admin(&mut conn, &caller)
        .await
        .map(Some)
}

impl Setup {
    /// The command checks every dashboard command gets, then the body.
    fn admit<T: DeserializeOwned>(&self, headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
        check_origin(&self.config, headers)?;
        serde_json::from_slice(body)
            .map_err(|e| ApiError::with_message(ErrorCode::MalformedRequest, e.to_string()))
    }

    /// The token authorizes until an admin exists.
    fn verify(&self, token: &str) -> Result<(), ApiError> {
        if self.claimed.load(Ordering::SeqCst) {
            return Err(ApiError::new(ErrorCode::SetupRequired));
        }
        let mut failures = self.failures.lock().expect("setup limiter lock");
        let now = Instant::now();
        let (since, count) = failures
            .filter(|(since, _)| now.duration_since(*since) < Duration::from_secs(60))
            .unwrap_or((now, 0));
        if count >= ATTEMPTS_PER_MINUTE {
            return Err(ApiError::new(ErrorCode::RateLimited));
        }
        if token_digest(&normalize(token)) == self.token {
            return Ok(());
        }
        *failures = Some((since, count + 1));
        Err(ApiError::new(ErrorCode::SetupTokenInvalid))
    }

    /// The control database of the import phase.
    async fn installed(&self) -> Result<Db, ApiError> {
        match &*self.phase.lock().await {
            Phase::Import(installed) => Ok(installed.db.clone()),
            Phase::Claim | Phase::Restoring | Phase::Done => {
                Err(ApiError::new(ErrorCode::SetupRequired))
            }
        }
    }

    /// The import phase's database and the signed-in install admin, `unauthenticated` without
    /// a session.
    async fn authorized(
        &self,
        headers: &HeaderMap,
        peer: SocketAddr,
    ) -> Result<(Db, UserId), ApiError> {
        let db = self.installed().await?;
        let user = admin_session(&db, headers, peer)
            .await?
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
        Ok((db, user))
    }
}

/// A server-sent event stream of `frames`: the current one, then one per change, until setup
/// mode stops.
fn frames_stream<T>(setup: &Setup, frames: watch::Receiver<T>) -> Response
where
    T: Serialize + Clone + Send + Sync + 'static,
{
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
            let event = axum::response::sse::Event::default()
                .json_data(&frame)
                .expect("frames serialize");
            Some((
                Ok::<_, std::convert::Infallible>(event),
                (frames, closing, false),
            ))
        },
    );
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// Upper case without dashes or spaces, so a token typed by hand still matches.
fn normalize(token: &str) -> String {
    token
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Four groups of four from an alphabet without look-alikes (0/O, 1/I/L): 80 bits.
fn generate_token() -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let mut groups = Vec::with_capacity(TOKEN_GROUPS);
    for _ in 0..TOKEN_GROUPS {
        let mut group = String::with_capacity(TOKEN_GROUP_CHARS);
        while group.len() < TOKEN_GROUP_CHARS {
            let mut byte = [0u8; 1];
            getrandom::fill(&mut byte).expect("the OS provides randomness");
            // 248 is the largest multiple of 31 below 256, so every character is equally likely.
            if byte[0] < 248 {
                group.push(ALPHABET[(byte[0] % 31) as usize] as char);
            }
        }
        groups.push(group);
    }
    groups.join("-")
}

/// The data directory may not exist yet; its volume does.
fn existing_ancestor(dir: &Path) -> &Path {
    dir.ancestors()
        .find(|path| path.exists())
        .unwrap_or(Path::new("/"))
}

fn is_private(address: std::net::IpAddr) -> bool {
    match address {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_tokens_match_the_logged_one() {
        let token = generate_token();
        assert_eq!(token.len(), 19);
        let typed = format!(" {} ", token.to_lowercase().replace('-', " "));
        assert_eq!(normalize(&typed), normalize(&token));
    }
}
