use std::future::IntoFuture;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use holocron_server::background::Background;
use holocron_server::config::{Config, ServeArgs};
use holocron_server::init::{self, Exit, Failure, InitArgs};
use holocron_server::maintenance::commands::{
    self, BackupArgs, DataDir, EraseProfileArgs, RestoreArgs, UpgradeArgs,
};
use holocron_server::maintenance::lock::DataLock;
use holocron_server::maintenance::{Runtime, versions};
use holocron_server::router;
use holocron_server::setup::Outcome;
use serde_json::Value;
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

/// How long open requests may take to finish after a stop signal. Live subscriptions never
/// end by themselves, so they are cut here.
const DRAIN: Duration = Duration::from_secs(10);

#[derive(Parser)]
#[command(
    name = "holocron",
    version = env!("HOLOCRON_VERSION"),
    about = "Self-hosted analytics for Umami operators"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Creates a data directory with the first install admin.
    Init {
        #[arg(long, env = "HOLOCRON_DATA_DIR")]
        data_dir: PathBuf,
        #[arg(long)]
        admin_email: String,
        /// File holding the admin password. Without it a password is generated and printed once.
        #[arg(long)]
        admin_password_file: Option<PathBuf>,
    },
    /// Serves the dashboard, its API and the web app.
    Serve(Box<ServeArgs>),
    /// Inspects or imports an Umami 3.4.0 PostgreSQL or 2.20.2 MySQL database.
    Import {
        #[command(subcommand)]
        command: holocron_server::imports::Command,
    },
    /// Writes a verified backup of every engine, the secret key and a manifest. A running
    /// server pauses analytical writes for the copy; collection goes on.
    Backup(BackupArgs),
    /// Restores a backup into a new data directory and checks it before it is served.
    Restore(RestoreArgs),
    /// Migrates a stopped installation to this release, after a pre-upgrade backup.
    Upgrade(UpgradeArgs),
    /// Removes data past retention now instead of at the next hourly run.
    Retention(DataDir),
    /// Erases one consented visitor profile's history: events, identifies, sessions, replays.
    EraseProfile(EraseProfileArgs),
    /// Prints operational state as JSON, without event data, identity inputs or credentials.
    Diagnostics(DataDir),
    /// Posts a JSON query to a running server and prints the answer. The API key comes from
    /// HOLOCRON_API_KEY. Does not open the data directory.
    Query(holocron_server::query_cli::QueryArgs),
}

fn main() -> ExitCode {
    limit_allocator_arenas();
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("HOLOCRON_LOG")
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();

    let workers = match &cli.command {
        Command::Serve(args) => args.sizing().worker_threads,
        Command::Init { .. } | Command::Import { .. } | Command::Query(_) => 1,
        _ => 2,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return fail(Failure::new(Exit::Internal, "internal", error.to_string()));
        }
    };
    let result = runtime.block_on(async {
        match cli.command {
            Command::Init {
                data_dir,
                admin_email,
                admin_password_file,
            } => run_init(data_dir, admin_email, admin_password_file).await,
            Command::Serve(args) => serve(*args).await,
            Command::Import { command } => holocron_server::imports::cli(command).await,
            Command::Backup(args) => print(commands::backup(args).await),
            Command::Restore(args) => print(commands::restore(args).await),
            Command::Upgrade(args) => print(commands::upgrade(args).await),
            Command::Retention(args) => print(commands::retention(args).await),
            Command::EraseProfile(args) => print(commands::erase_profile(args).await),
            Command::Diagnostics(args) => print(commands::diagnostics(args).await),
            Command::Query(args) => {
                let key = std::env::var(holocron_server::query_cli::KEY_ENV).ok();
                print(
                    tokio::task::spawn_blocking(move || {
                        holocron_server::query_cli::run(&args, key)
                    })
                    .await
                    .expect("the query task does not panic"),
                )
            }
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => fail(failure),
    }
}

/// glibc creates up to eight malloc arenas per core; with DuckDB and Tokio threads that was
/// part of the out-of-memory runs in milestone 02. Two arenas fit the 4 GiB budget (0008).
fn limit_allocator_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: called first in main, before any thread exists.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

fn fail(failure: Failure) -> ExitCode {
    eprintln!("error: {}: {}", failure.code, failure.message);
    ExitCode::from(failure.exit as u8)
}

/// An operator command's result, as JSON on stdout.
fn print(result: Result<Value, Failure>) -> Result<(), Failure> {
    let value = result?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("results serialize")
    );
    Ok(())
}

async fn run_init(
    data_dir: PathBuf,
    admin_email: String,
    password_file: Option<PathBuf>,
) -> Result<(), Failure> {
    let admin_password = match password_file {
        Some(path) => Some(
            std::fs::read_to_string(&path)
                .map(|text| text.trim_end_matches(['\n', '\r']).to_owned())
                .map_err(|e| {
                    Failure::new(Exit::Usage, "usage", format!("{}: {e}", path.display()))
                })?,
        ),
        None => None,
    };
    let done = init::init(InitArgs {
        data_dir: data_dir.clone(),
        admin_email,
        admin_password,
    })
    .await?;
    eprintln!("initialized {}", data_dir.display());
    if let Some(password) = done.generated_password {
        // stdout only, once. Logs and stderr never see it.
        println!("{password}");
    }
    Ok(())
}

/// Serves until SIGTERM or Ctrl-C, then stops in order: no new connections, open requests
/// get [`DRAIN`] to finish, background work commits its step, then storage stops admission,
/// lets transfer finish its batch, closes the read workers and checkpoints. The next start
/// reconciles from the ledger.
async fn serve(args: ServeArgs) -> Result<(), Failure> {
    let usage = |e: anyhow::Error| Failure::new(Exit::Usage, "usage", e.to_string());
    let clock = holocron_server::config::clock(args.frozen_clock.as_deref()).map_err(usage)?;
    let sizing = args.sizing();
    let config = Config::from_args(args).map_err(usage)?;
    let dir = config.data_dir.clone();
    let bind = config.bind;
    let bind_failed =
        |e: std::io::Error| Failure::new(Exit::Internal, "bind_failed", format!("{bind}: {e}"));
    // One socket for setup mode and the server after it, so the port never closes in between.
    let socket = std::net::TcpListener::bind(bind).map_err(bind_failed)?;
    socket.set_nonblocking(true).map_err(bind_failed)?;

    let (stop, stopped) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop.send(true);
    });

    // An empty directory, or an import the wizard has not finished, starts in setup mode.
    if holocron_server::setup::required(&dir).await? {
        let listener = socket
            .try_clone()
            .and_then(tokio::net::TcpListener::from_std)
            .map_err(bind_failed)?;
        match holocron_server::setup::run(config.clone(), clock.clone(), listener, stopped.clone())
            .await?
        {
            Outcome::Initialized => {}
            Outcome::Stopped => return Ok(()),
        }
    }
    init::check_initialized(&dir)?;
    let _lock = DataLock::acquire(&dir)?;
    versions::check_dir(&dir).await?;

    match &config.web_dir {
        Some(dir) => tracing::info!(dir = %dir.display(), "serving the web app"),
        None => tracing::warn!("no web app found; set HOLOCRON_WEB_DIR to serve the dashboard"),
    }
    if config.public_origin.is_none() {
        tracing::warn!(
            "HOLOCRON_PUBLIC_URL is not set: commands are checked against the Host header and the session cookie is not marked Secure"
        );
    }

    let listener = tokio::net::TcpListener::from_std(socket).map_err(bind_failed)?;
    let budgets = sizing.storage;
    tracing::info!(
        budget = ?sizing.budget,
        tokio_workers = sizing.worker_threads,
        engine_threads = budgets.engine_threads,
        engine_memory_mib = budgets.engine_memory_bytes >> 20,
        report_workers = budgets.report_workers,
        report_queue = budgets.report_queue,
        spill_gib = budgets.spill_bytes >> 30,
        inbox_gib = budgets.inbox_bytes >> 30,
        "resource budget"
    );
    if clock.is_test() {
        tracing::warn!(
            now_ms = clock.now_ms(),
            "the application clock is frozen; development builds only"
        );
    }
    let runtime = Runtime::open(config, budgets, clock).await?;
    let state = runtime.state.clone();
    let background = Background::start(&state);
    tracing::info!(addr = %bind, version = env!("CARGO_PKG_VERSION"), "holocron listening");

    let signalled = state.clone();
    let mut stopping = stopped.clone();
    tokio::spawn(async move {
        let _ = stopping.wait_for(|stop| *stop).await;
        // Readiness answers unavailable from here, while open requests finish.
        signalled.maintenance.stop();
    });
    let until_stopped = |mut stopped: watch::Receiver<bool>| async move {
        let _ = stopped.wait_for(|stop| *stop).await;
    };
    let server = axum::serve(
        listener,
        router(state.clone()).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(until_stopped(stopped.clone()))
    .into_future();
    let served = tokio::select! {
        result = server => result.map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string())),
        _ = async {
            until_stopped(stopped).await;
            tokio::time::sleep(DRAIN).await;
        } => {
            tracing::warn!("requests still open {DRAIN:?} after the stop signal; closing them");
            Ok(())
        }
    };

    tracing::info!("stopping: background work, then admission and transfer");
    background.stop().await;
    let status = state.collector.storage().status();
    drop(state);
    runtime.close().await?;
    tracing::info!(
        watermark_batch = status.watermark.batch,
        inbox_records = status.inbox_records,
        "stopped; waiting records transfer at the next start"
    );
    served
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
