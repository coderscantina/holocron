//! The setup wizard's endpoints under `/api/setup/`, which answer only while the server runs in
//! setup mode (decision 0021). They are not operations: setup mode has no runtime for the
//! registry's handlers to run against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::operations::auth::Account;
use crate::retention::Retention;

/// `POST /api/setup/check`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupTokenArgs {
    /// The token the server logged at start, or `HOLOCRON_SETUP_TOKEN`. Case, spaces and dashes
    /// do not matter.
    pub token: String,
}

/// What the server sees of its deployment. The wizard shows a warning for each problem and
/// lets the operator continue.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupCheck {
    pub version: String,
    /// The origin of `HOLOCRON_PUBLIC_URL`, absent when it is not set. The wizard compares it
    /// with the address it was opened at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_origin: Option<String>,
    /// The caller's address as collection would record it, after trusted proxies.
    pub visitor_address: String,
    /// The address is loopback or private: behind a proxy that Holocron does not trust, every
    /// visitor would get it.
    pub visitor_address_private: bool,
    pub disk_free_bytes: u64,
    /// Collection refuses events below this (`HOLOCRON_DISK_RESERVE_MIB`).
    pub disk_reserve_bytes: u64,
}

/// `POST /api/setup/admin`: creates the data directory with its first install admin, the
/// Default team and the secrets, as `holocron init` does, then signs the admin in. Answers
/// `auth.login`'s `signed_in` result with the session cookie set. On the fresh path the server
/// leaves setup mode right after; on the import path it stays until the import report is
/// acknowledged.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupAdminArgs {
    pub token: String,
    pub email: String,
    pub password: String,
    /// `fresh` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<SetupAdminPath>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupAdminPath {
    #[default]
    Fresh,
    Import,
}

/// `POST /api/setup/sign-in`: signs the install admin in again while setup mode waits for an
/// import, for example after a restart. Only the admin setup created qualifies, an account with
/// a Holocron password and no second factor. Imported Umami accounts sign in after setup.
/// Answers `auth.login`'s `signed_in` result with the session cookie set.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupSignInArgs {
    pub email: String,
    pub password: String,
}

/// Where setup stands. `claim`: no admin yet, the setup token authorizes. `import`: the admin
/// exists and an Umami import waits for the wizard; the admin's session authorizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupPhase {
    Claim,
    Import,
}

/// `GET /api/setup/status`. In the import phase without a session only `phase` is set.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupStatus {
    pub phase: SetupPhase,
    /// A restore stopped partway and left its directory behind. The wizard offers the restore
    /// step again; running it starts over.
    #[serde(default)]
    pub restore_incomplete: bool,
    /// The signed-in install admin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<Account>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub import: Option<SetupImportStatus>,
}

/// How far the import is. `interrupted` is a started run that no job continues: the server
/// restarted, or the job stopped with an error. It resumes with the same source and options.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupImportState {
    #[default]
    None,
    Running,
    Interrupted,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportStatus {
    pub state: SetupImportState,
    /// The source layout a started run reads, such as `umami-3.4.0-postgresql`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
    /// The options a started run keeps. A resume sends the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub options: Option<SetupImportOptions>,
    /// Why the last job of this server process stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<SetupImportError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report: Option<SetupImportReport>,
}

/// An import refusal or failure: the importer's code, such as `source_changed` or
/// `credentials_not_read_only`, and its message, which says how to fix it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportError {
    pub code: String,
    pub message: String,
}

/// The options of `holocron import run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportOptions {
    /// IANA timezone of every imported site.
    pub timezone: String,
    pub retention: Retention,
    /// Logins of Umami usernames that are not email addresses become `<username>@<domain>`.
    pub login_domain: String,
    /// Source rows per chunk, 1 to 100000.
    pub chunk_rows: i64,
}

/// `POST /api/setup/import/inspect`. The source URL is held in memory only, here and in a run.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupInspectArgs {
    /// `postgres://` or `mysql://` URL, ideally of a read-only user and, off a private
    /// network, asking for TLS. Without either the preview carries a warning.
    pub source: String,
    /// The timezone the retention preview counts days in.
    pub timezone: String,
}

/// What `holocron import inspect` reports: the preview, or the refusal with its fix.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupInspection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub preview: Option<SetupImportPreview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refusal: Option<SetupImportError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportPreview {
    /// The source layout, such as `umami-3.4.0-postgresql`.
    pub source: String,
    /// Rows per source table.
    pub tables: Vec<SetupTableRows>,
    pub events: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub first_event: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_event: Option<i64>,
    /// What each retention choice leaves behind, at today's cutoffs.
    pub retention: Vec<SetupRetentionPreview>,
    /// An upper bound for the disk the import needs.
    pub source_bytes: i64,
    /// What the import proceeds with despite the risk.
    #[serde(default)]
    pub warnings: Vec<SetupImportWarning>,
}

/// A risk the import accepts, with a message that says how to remove it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportWarning {
    pub code: SetupImportWarningCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupImportWarningCode {
    /// The host is off a private network and the URL doesn't require TLS.
    SourceUnencrypted,
    /// The credentials could change the source. The import's session reads only.
    CredentialsWritable,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupTableRows {
    pub table: String,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRetentionPreview {
    pub retention: Retention,
    /// Events before the cutoff, which are not imported.
    pub events_outside: i64,
    /// Absent for `forever`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cutoff: Option<i64>,
}

/// `POST /api/setup/import/run`: starts the import, or resumes an interrupted one with the same
/// source and options. Progress follows on `GET /api/setup/import/progress`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRunArgs {
    pub source: String,
    pub options: SetupImportOptions,
    /// Umami's `TWO_FACTOR_ENCRYPTION_KEY`, 64 hex characters. Held in memory only; without it
    /// accounts with 2FA enroll again at their first sign-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub two_factor_key: Option<String>,
}

/// The import steps in order. `finishing` is session correction and reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupImportStepName {
    Control,
    Events,
    Identify,
    Heatmaps,
    SessionLinks,
    Replays,
    Finishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupStepState {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportStep {
    pub step: SetupImportStepName,
    pub state: SetupStepState,
    /// Source rows read so far, including those of earlier runs.
    pub rows: i64,
    /// Rows of the step's source table when the run started, 0 for steps without rows.
    pub total: i64,
    /// Seconds left at the rate of the last half minute, once there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub eta_seconds: Option<i64>,
}

/// One frame of `GET /api/setup/import/progress`, a server-sent event stream. The first frame
/// is the current state, so a reloaded page reattaches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportProgress {
    pub state: SetupImportState,
    pub steps: Vec<SetupImportStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<SetupImportError>,
}

/// The parts of the import manifest the report step shows. The whole manifest is
/// `GET /api/setup/import/manifest`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportReport {
    pub reconciled: bool,
    /// Each count, range or total that differs from the source.
    pub differences: Vec<String>,
    pub sites: Vec<SetupImportedSite>,
    pub logins: Vec<SetupImportedLogin>,
    /// Source objects not imported, or imported with a loss, by table and reason.
    pub inventory: Vec<SetupInventoryCount>,
    /// Events not imported by reason, such as `outside_retention`. `table` is empty.
    pub events_not_imported: Vec<SetupInventoryCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportedSite {
    pub id: String,
    /// `site`, `link` or `pixel`.
    pub kind: String,
    pub name: String,
    /// Imported events of every kind.
    pub events: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupImportedLogin {
    pub username: String,
    pub email: String,
    /// `kept` or `reset_required`.
    pub password: String,
    /// `kept`, `enrollment_required` or `none`.
    pub two_factor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupInventoryCount {
    pub table: String,
    pub reason: String,
    pub count: i64,
}

/// `POST /api/setup/import/discard`: removes the imported data and keeps the admin and the
/// secret key. `confirm` is the instance name.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupDiscardArgs {
    pub confirm: String,
}

/// `POST /api/setup/restore/backups`: the backups in the mounted backup directory
/// (`HOLOCRON_BACKUP_DIR`, `/backups` in the compose files), newest first.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupBackups {
    /// The directory that was read.
    pub directory: String,
    /// Backups with a readable manifest. Their files are checked when the restore starts.
    pub backups: Vec<SetupBackup>,
    /// A restore in progress, so a reloaded wizard can attach to it.
    pub restore: SetupRestoreProgress,
}

/// A backup as its manifest describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupBackup {
    /// Absolute path on the server's host.
    pub path: String,
    pub created_at_ms: i64,
    /// Size of its files.
    pub bytes: u64,
    /// The release that took it.
    pub release: String,
    /// It comes from an older release and is upgraded right after the restore.
    pub upgrade: bool,
}

/// `POST /api/setup/restore/inspect`: reads the manifest of a backup at any absolute host path.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRestoreInspectArgs {
    pub token: String,
    pub path: String,
}

/// The backup, or why it can't be restored here.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRestoreInspection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub backup: Option<SetupBackup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refusal: Option<SetupImportError>,
}

/// `POST /api/setup/restore/start`: checks every file of the backup against its manifest,
/// restores it into the data directory and upgrades it when it comes from an older release.
/// Progress follows on `GET /api/setup/restore/progress`. When it finishes, the server leaves
/// setup mode and the restored accounts sign in. Answers the first progress frame.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRestoreArgs {
    pub token: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupRestoreState {
    #[default]
    Idle,
    Running,
    /// Both phases are done and the server is starting.
    Complete,
    /// The restore stopped; `error` says why. The step can run again.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupRestorePhase {
    /// Checking the files and copying them.
    Restoring,
    /// Migrating the restored data to this release.
    Upgrading,
}

/// One frame of `GET /api/setup/restore/progress`, a server-sent event stream. The token goes in
/// the `X-Holocron-Setup-Token` header. The first frame is the current state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SetupRestoreProgress {
    pub state: SetupRestoreState,
    /// The running phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub phase: Option<SetupRestorePhase>,
    /// The backup is older than this release, so an upgrade phase follows the restore.
    #[serde(default)]
    pub upgrade: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<SetupImportError>,
}
