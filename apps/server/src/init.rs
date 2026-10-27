//! `holocron init` and the data-directory state `serve` requires (decision 0006).

use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use holocron_protocol::access::{InstallRole, Role, WorkspaceKind};
use holocron_protocol::ids::UserId;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

use crate::db::{CONTROL_DB_FILE, Db};
use crate::identity::{self, MIN_PASSWORD_CHARS};
use crate::sites;

pub const IN_PROGRESS_MARKER: &str = "holocron.init-in-progress";
pub const DONE_MARKER: &str = "holocron.initialized";
/// The setup wizard chose the import path: `serve` comes back to the wizard until the import
/// report is acknowledged, even after a restart before the import started (decision 0021).
pub const SETUP_IMPORT_MARKER: &str = "holocron.setup-import";
/// The control database of a discarded import while [`reset`] copies the admin out of it.
const DISCARDED_CONTROL_DB: &str = "holocron.discarded-control.sqlite";

/// Files of the engines, which an interrupted init or a reset removes.
const ENGINE_FILES: [&str; 12] = [
    CONTROL_DB_FILE,
    "control.sqlite-wal",
    "control.sqlite-shm",
    "inbox.sqlite",
    "inbox.sqlite-wal",
    "inbox.sqlite-shm",
    "analytics.duckdb",
    "analytics.duckdb.wal",
    "recordings.sqlite",
    "recordings.sqlite-wal",
    "recordings.sqlite-shm",
    DISCARDED_CONTROL_DB,
];

/// Name of the workspace init creates for the first admin.
const FIRST_WORKSPACE: &str = "Default";

/// Exit codes shared by every `holocron` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    Internal = 1,
    Usage = 2,
    Refused = 3,
    StorageUnavailable = 4,
    /// A running server answered `holocron query` with an error envelope.
    ServerRefused = 5,
}

#[derive(Debug)]
pub struct Failure {
    pub exit: Exit,
    /// A fixed outcome name, or one a server job reported back.
    pub code: Cow<'static, str>,
    pub message: String,
}

impl Failure {
    pub fn new(exit: Exit, code: impl Into<Cow<'static, str>>, message: impl Into<String>) -> Self {
        Self {
            exit,
            code: code.into(),
            message: message.into(),
        }
    }

    pub(crate) fn storage(path: &Path, error: impl std::fmt::Display) -> Self {
        Self::new(
            Exit::StorageUnavailable,
            "storage_unavailable",
            format!("{}: {error}", path.display()),
        )
    }
}

pub struct InitArgs {
    pub data_dir: PathBuf,
    pub admin_email: String,
    pub admin_password: Option<String>,
}

/// What a successful init reports. `generated_password` is printed once and stored nowhere.
pub struct Initialized {
    pub generated_password: Option<String>,
    pub admin: UserId,
}

pub async fn init(args: InitArgs) -> Result<Initialized, Failure> {
    init_with(args, false).await
}

/// [`init`] for the setup wizard's import path. It writes [`SETUP_IMPORT_MARKER`] before the
/// directory counts as initialized, so no restart finds it set up without its import.
pub(crate) async fn init_for_import(args: InitArgs) -> Result<Initialized, Failure> {
    init_with(args, true).await
}

async fn init_with(args: InitArgs, import: bool) -> Result<Initialized, Failure> {
    let email = identity::normalize_email(&args.admin_email);
    if !email.contains('@') {
        return Err(Failure::new(
            Exit::Usage,
            "usage",
            "--admin-email must be an email address",
        ));
    }
    if let Some(password) = &args.admin_password
        && password.chars().count() < MIN_PASSWORD_CHARS
    {
        return Err(Failure::new(
            Exit::Usage,
            "usage",
            format!("the admin password needs at least {MIN_PASSWORD_CHARS} characters"),
        ));
    }

    let dir = &args.data_dir;
    prepare_directory(dir)?;
    let _lock = crate::maintenance::lock::DataLock::acquire(dir)?;
    fs::write(dir.join(IN_PROGRESS_MARKER), b"").map_err(|e| Failure::storage(dir, e))?;

    let generated_password = args
        .admin_password
        .is_none()
        .then(identity::generate_password);
    let password = args
        .admin_password
        .as_deref()
        .or(generated_password.as_deref())
        .unwrap_or_default();
    let name = email.split('@').next().unwrap_or_default().to_owned();

    let db = Db::open(dir).await.map_err(|e| Failure::storage(dir, e))?;
    let created = async {
        let mut tx = db.writer.begin_with("BEGIN IMMEDIATE").await?;
        let user =
            identity::create_user(&mut tx, &email, &name, password, InstallRole::InstallAdmin)
                .await?;
        first_workspace(&mut tx, &user).await?;
        tx.commit().await?;
        crate::pseudonyms::root_secret(&db, crate::db::now_ms()).await?;
        Ok::<_, sqlx::Error>(user)
    }
    .await;
    db.close().await;
    let admin = created.map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string()))?;
    let marker = dir.join(SETUP_IMPORT_MARKER);
    match import {
        true => fs::write(&marker, b"").map_err(|e| Failure::storage(dir, e))?,
        false => remove_files(dir, &[SETUP_IMPORT_MARKER])?,
    }
    complete(dir).await?;
    Ok(Initialized {
        generated_password,
        admin,
    })
}

/// The Default team, owned by the first admin.
async fn first_workspace(conn: &mut SqliteConnection, admin: &UserId) -> sqlx::Result<()> {
    let workspace =
        sites::create_workspace(&mut *conn, FIRST_WORKSPACE, WorkspaceKind::Team).await?;
    sites::add_member(conn, &workspace, admin, Role::Owner).await
}

/// Creates the secret key and the other engine files, then marks the directory initialized.
async fn complete(dir: &Path) -> Result<(), Failure> {
    // The key for second-factor secrets lives beside the database, never in it (decision 0012).
    identity::secrets::SecretKey::load_or_create(dir).map_err(|e| Failure::storage(dir, e))?;
    // The other engine files, so every initialized directory has the whole set at this
    // release's schema and can be backed up and upgraded before it was ever served.
    holocron_storage::Storage::open(dir, holocron_storage::Budgets::default())
        .await
        .map_err(|e| Failure::storage(dir, e))?
        .close()
        .await
        .map_err(|e| Failure::storage(dir, e))?;
    crate::recordings::store::Store::open(dir)
        .await
        .map_err(|e| Failure::storage(dir, e))?
        .close()
        .await;

    fs::write(dir.join(DONE_MARKER), b"").map_err(|e| Failure::storage(dir, e))?;
    fs::remove_file(dir.join(IN_PROGRESS_MARKER)).map_err(|e| Failure::storage(dir, e))
}

/// Tables of the kept admin besides `users` that [`reset`] copies: sign-in and second factor.
const KEPT_ADMIN_TABLES: [&str; 4] = [
    "sessions",
    "two_factor",
    "recovery_codes",
    "user_preferences",
];

/// Puts an initialized directory back to what init made, keeping `secrets.key` and the install
/// admin `keep` with its sessions and second factor: "Discard import and start over" (decision
/// 0021). The caller holds the lock and has closed every database of `dir`. Interrupted, the
/// directory reads as `init_incomplete`, and setup starts over at the admin step.
pub(crate) async fn reset(dir: &Path, keep: &UserId) -> Result<(), Failure> {
    let failed = |e: &dyn std::fmt::Display| Failure::storage(dir, e);
    fs::write(dir.join(IN_PROGRESS_MARKER), b"").map_err(|e| failed(&e))?;
    remove_files(dir, &[DONE_MARKER])?;

    // Leaving WAL mode folds the log into the file, so it can move on its own.
    let options = SqliteConnectOptions::new().filename(dir.join(CONTROL_DB_FILE));
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .map_err(|e| failed(&e))?;
    sqlx::query("PRAGMA journal_mode = DELETE")
        .execute(&mut conn)
        .await
        .map_err(|e| failed(&e))?;
    conn.close().await.map_err(|e| failed(&e))?;
    let discarded = dir.join(DISCARDED_CONTROL_DB);
    fs::rename(dir.join(CONTROL_DB_FILE), &discarded).map_err(|e| failed(&e))?;
    remove_files(dir, &ENGINE_FILES[..ENGINE_FILES.len() - 1])?;

    let db = Db::open(dir).await.map_err(|e| failed(&e))?;
    let copied = async {
        let mut conn = db.writer.acquire().await?;
        sqlx::query("ATTACH DATABASE ? AS discarded")
            .bind(discarded.to_string_lossy().into_owned())
            .execute(&mut *conn)
            .await?;
        sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
        sqlx::query("INSERT INTO users SELECT * FROM discarded.users WHERE id = ?")
            .bind(keep.as_str())
            .execute(&mut *conn)
            .await?;
        for table in KEPT_ADMIN_TABLES {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {table} SELECT * FROM discarded.{table} WHERE user_id = ?"
            )))
            .bind(keep.as_str())
            .execute(&mut *conn)
            .await?;
        }
        first_workspace(&mut conn, keep).await?;
        sqlx::query("COMMIT").execute(&mut *conn).await?;
        sqlx::query("DETACH DATABASE discarded")
            .execute(&mut *conn)
            .await?;
        drop(conn);
        crate::pseudonyms::root_secret(&db, crate::db::now_ms()).await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;
    db.close().await;
    copied.map_err(|e| Failure::new(Exit::Internal, "internal", e.to_string()))?;
    fs::remove_file(&discarded).map_err(|e| failed(&e))?;
    complete(dir).await
}

/// Removes `files` from `dir` where they exist.
fn remove_files(dir: &Path, files: &[&str]) -> Result<(), Failure> {
    for file in files {
        match fs::remove_file(dir.join(file)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(Failure::storage(dir, e));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Creates the directory, or clears the leftovers of an interrupted init.
fn prepare_directory(dir: &Path) -> Result<(), Failure> {
    if dir.join(DONE_MARKER).exists() {
        return Err(Failure::new(
            Exit::Refused,
            "already_initialized",
            dir.display().to_string(),
        ));
    }
    if dir.join(IN_PROGRESS_MARKER).exists() {
        return remove_files(dir, &ENGINE_FILES);
    }
    match fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(Failure::new(
                    Exit::Refused,
                    "directory_not_empty",
                    dir.display().to_string(),
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir).map_err(|e| Failure::storage(dir, e))
        }
        Err(e) => Err(Failure::storage(dir, e)),
    }
}

/// `serve` starts only on a directory init finished.
pub fn check_initialized(dir: &Path) -> Result<(), Failure> {
    if dir.join(DONE_MARKER).exists() {
        return Ok(());
    }
    let code = if dir.join(IN_PROGRESS_MARKER).exists() {
        "init_incomplete"
    } else if dir
        .join(crate::maintenance::restore::IN_PROGRESS_MARKER)
        .exists()
    {
        "restore_incomplete"
    } else {
        "not_initialized"
    };
    Err(Failure::new(
        Exit::Refused,
        code,
        format!(
            "{} is not an initialized data directory; run `holocron init`",
            dir.display()
        ),
    ))
}
