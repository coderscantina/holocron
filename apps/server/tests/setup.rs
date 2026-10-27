//! Setup mode on an empty data directory (decision 0021), over a real socket because the
//! wizard hands its listener to `serve` when it ends.

mod common;

use std::net::SocketAddr;
use std::path::Path;

use clap::Parser;
use common::wire;
use holocron_protocol::access::InstallRole;
use holocron_server::clock::Clock;
use holocron_server::config::{Config, ServeArgs};
use holocron_server::db::Db;
use holocron_server::init::{self, InitArgs};
use holocron_server::maintenance::backup::{self, MANIFEST_FILE, Source};
use holocron_server::setup::{self, Outcome};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::task::JoinHandle;

const TOKEN: &str = "ABCD-EFGH-JKMN-PQRS";

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    args: ServeArgs,
}

struct Reply {
    status: u16,
    head: String,
    body: Value,
}

/// One HTTP/1.1 request with `Connection: close`, as a browser on this origin sends it.
async fn request(addr: SocketAddr, method: &str, path: &str, body: Option<Value>) -> Reply {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\nX-Holocron-Request: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    // Small answers arrive in one chunk; drop the chunk framing if there is any.
    let json = body
        .lines()
        .find(|line| line.starts_with('{'))
        .map(|line| serde_json::from_str(line).unwrap())
        .unwrap_or(Value::Null);
    Reply {
        status,
        head: head.to_owned(),
        body: json,
    }
}

#[tokio::test]
async fn the_wizard_creates_the_admin_and_hands_over() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let args = Cli::try_parse_from([
        "holocron",
        "--data-dir",
        data.to_str().unwrap(),
        "--setup-token",
        &TOKEN.to_lowercase(),
        "--disk-reserve-mib",
        "0",
    ])
    .unwrap()
    .args;
    let config = Config::from_args(args).unwrap();
    assert!(setup::needed(&data).unwrap());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (_stop, stopped) = watch::channel(false);
    let run = tokio::spawn(setup::run(config, Clock::system(), listener, stopped));

    let config = request(addr, "GET", "/config.json", None).await;
    assert_eq!(config.body["setup"], true);
    let collect = request(addr, "POST", "/api/send", Some(json!({}))).await;
    assert_eq!(
        (collect.status, collect.body["error_code"].as_str()),
        (503, Some("setup_required"))
    );
    let wrong = request(
        addr,
        "POST",
        "/api/setup/check",
        Some(json!({ "token": "nope" })),
    )
    .await;
    assert_eq!(
        (wrong.status, wrong.body["error_code"].as_str()),
        (401, Some("setup_token_invalid"))
    );
    let check = request(
        addr,
        "POST",
        "/api/setup/check",
        Some(json!({ "token": "abcd efgh jkmn pqrs" })),
    )
    .await;
    assert_eq!(check.status, 200);
    assert_eq!(check.body["visitor_address"], "127.0.0.1");
    assert_eq!(check.body["visitor_address_private"], true);

    let admin = request(
        addr,
        "POST",
        "/api/setup/admin",
        Some(json!({ "token": TOKEN, "email": "Ops@Example.org", "password": "correct horse battery" })),
    )
    .await;
    assert_eq!(admin.status, 200, "{}", admin.body);
    assert_eq!(admin.body["status"], "signed_in");
    assert_eq!(admin.body["account"]["email"], "ops@example.org");
    assert_eq!(admin.body["account"]["install_role"], "install_admin");
    assert!(admin.head.contains("holocron_session="));

    assert!(matches!(run.await.unwrap(), Ok(Outcome::Initialized)));
    holocron_server::init::check_initialized(&data).unwrap();
    assert!(!setup::needed(&data).unwrap());
}

struct Running {
    addr: SocketAddr,
    run: JoinHandle<Result<Outcome, holocron_server::init::Failure>>,
    stop: watch::Sender<bool>,
}

/// Setup mode on `data` with the test token, ready for requests.
async fn start(data: &Path) -> Running {
    start_with_backups(data, Path::new("/nonexistent/backups")).await
}

/// [`start`] with `backups` as the backup directory the wizard lists.
async fn start_with_backups(data: &Path, backups: &Path) -> Running {
    let args = Cli::try_parse_from([
        "holocron",
        "--data-dir",
        data.to_str().unwrap(),
        "--setup-token",
        TOKEN,
        "--disk-reserve-mib",
        "0",
        "--backup-dir",
        backups.to_str().unwrap(),
    ])
    .unwrap()
    .args;
    let config = Config::from_args(args).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = watch::channel(false);
    let run = tokio::spawn(setup::run(config, Clock::system(), listener, stopped));
    wire::ready(addr).await;
    Running { addr, run, stop }
}

impl Running {
    async fn stop(self) -> Outcome {
        self.stop.send(true).unwrap();
        self.run.await.unwrap().unwrap()
    }
}

/// The admin step on the import path, answering the session cookie.
async fn import_admin(addr: SocketAddr) -> String {
    let admin = wire::request(
        addr,
        "POST",
        "/api/setup/admin",
        Some(json!({ "token": TOKEN, "email": "ops@example.org", "password": "correct horse battery", "path": "import" })),
        None,
    )
    .await;
    assert_eq!(admin.status, 200, "{}", admin.body);
    admin.session().unwrap()
}

#[tokio::test]
async fn the_import_path_stays_in_setup_mode_and_returns_to_it_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let server = start(&data).await;
    let addr = server.addr;
    let cookie = import_admin(addr).await;

    // The admin exists, but setup goes on: collection waits and the token is spent.
    assert!(!server.run.is_finished());
    let collect = wire::request(addr, "POST", "/api/send", Some(json!({})), None).await;
    assert_eq!(
        (collect.status, collect.code()),
        (503, Some("setup_required"))
    );
    let check = json!({ "token": TOKEN });
    let spent = wire::request(addr, "POST", "/api/setup/check", Some(check.clone()), None).await;
    assert_eq!((spent.status, spent.code()), (503, Some("setup_required")));

    // The phase is public; the import is the admin's.
    let anonymous = wire::request(addr, "GET", "/api/setup/status", None, None).await;
    assert_eq!(
        anonymous.body,
        json!({ "phase": "import", "restore_incomplete": false })
    );
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
    assert_eq!(status.body["account"]["email"], "ops@example.org");
    assert_eq!(status.body["import"]["state"], "none");
    let inspect =
        json!({ "source": "postgres://ro:pw@127.0.0.1:1/umami", "timezone": "Europe/Vienna" });
    let anonymous = wire::request(
        addr,
        "POST",
        "/api/setup/import/inspect",
        Some(inspect.clone()),
        None,
    )
    .await;
    assert_eq!(
        (anonymous.status, anonymous.code()),
        (401, Some("unauthenticated"))
    );

    // A source that can't be reached is refused with the importer's message.
    let refused = wire::request(
        addr,
        "POST",
        "/api/setup/import/inspect",
        Some(inspect),
        Some(&cookie),
    )
    .await;
    assert_eq!(refused.status, 200);
    assert_eq!(refused.body["refusal"]["code"], "source_unavailable");

    // A restart between the admin and the import comes back to the wizard, not to an empty
    // install. The session lives in the control database, and the admin can sign in again.
    assert!(matches!(server.stop().await, Outcome::Stopped));
    assert!(setup::needed(&data).unwrap());
    let server = start(&data).await;
    let addr = server.addr;
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
    assert_eq!(status.body["account"]["email"], "ops@example.org");
    let spent = wire::request(addr, "POST", "/api/setup/check", Some(check), None).await;
    assert_eq!(spent.code(), Some("setup_required"));
    let wrong = json!({ "email": "ops@example.org", "password": "not the password" });
    let wrong = wire::request(addr, "POST", "/api/setup/sign-in", Some(wrong), None).await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (401, Some("invalid_credentials"))
    );
    let right = json!({ "email": "OPS@example.org", "password": "correct horse battery" });
    let signed_in = wire::request(addr, "POST", "/api/setup/sign-in", Some(right), None).await;
    assert_eq!(signed_in.body["status"], "signed_in", "{}", signed_in.body);
    let fresh = signed_in.session().unwrap();
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&fresh)).await;
    assert_eq!(status.body["import"]["state"], "none");
    assert!(matches!(server.stop().await, Outcome::Stopped));
}

#[tokio::test]
async fn discarding_the_import_keeps_the_admin_and_setup_can_continue_without_one() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let server = start(&data).await;
    let addr = server.addr;
    let cookie = import_admin(addr).await;
    let secret = std::fs::read(data.join("secrets.key")).unwrap();

    // What an interrupted import leaves: an account it brought and its run.
    let db = Db::open(&data).await.unwrap();
    let mut conn = db.writer.acquire().await.unwrap();
    holocron_server::identity::create_user(
        &mut conn,
        "member@umami.invalid",
        "member",
        "umami password 1",
        InstallRole::User,
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO import_runs (id, source, fingerprint, options, clock_ms, state, started_at, updated_at)
         VALUES (1, 'umami-2.20.2-mysql', '{}', '{}', 0, 'running', 0, 0)",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    drop(conn);
    db.close().await;
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
    assert_eq!(status.body["import"]["state"], "interrupted");
    // Setup doesn't end on half an import.
    let finish = wire::request(
        addr,
        "POST",
        "/api/setup/finish",
        Some(json!({})),
        Some(&cookie),
    )
    .await;
    assert_eq!(
        (finish.status, finish.code()),
        (409, Some("import_in_progress"))
    );

    let name = wire::request(addr, "GET", "/config.json", None, None)
        .await
        .body["instance_name"]
        .clone();
    let wrong = json!({ "confirm": "not the name" });
    let wrong = wire::request(
        addr,
        "POST",
        "/api/setup/import/discard",
        Some(wrong),
        Some(&cookie),
    )
    .await;
    assert_eq!(
        (wrong.status, wrong.code()),
        (422, Some("invalid_arguments"))
    );
    let discard = json!({ "confirm": name });
    let discarded = wire::request(
        addr,
        "POST",
        "/api/setup/import/discard",
        Some(discard),
        Some(&cookie),
    )
    .await;
    assert_eq!(discarded.status, 200, "{}", discarded.body);

    // Still signed in, still in setup mode, with nothing imported.
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
    assert_eq!(status.body["account"]["email"], "ops@example.org");
    assert_eq!(status.body["import"]["state"], "none");
    assert_eq!(std::fs::read(data.join("secrets.key")).unwrap(), secret);
    let db = Db::connect(&data).await.unwrap();
    let users: Vec<String> = sqlx::query_scalar("SELECT email FROM users")
        .fetch_all(&db.reader)
        .await
        .unwrap();
    let teams: Vec<String> = sqlx::query_scalar("SELECT name FROM workspaces")
        .fetch_all(&db.reader)
        .await
        .unwrap();
    db.close().await;
    assert_eq!(
        (users, teams),
        (
            vec!["ops@example.org".to_owned()],
            vec!["Default".to_owned()]
        )
    );

    // Without a run, finishing continues without an import.
    let finish = wire::request(
        addr,
        "POST",
        "/api/setup/finish",
        Some(json!({})),
        Some(&cookie),
    )
    .await;
    assert_eq!(finish.status, 200, "{}", finish.body);
    assert!(matches!(
        server.run.await.unwrap(),
        Ok(Outcome::Initialized)
    ));
    assert!(!setup::needed(&data).unwrap());
}

const ADMIN: &str = "restored@example.org";
const PASSWORD: &str = "restored admin password";

/// A backup of a stopped installation with its own admin, in `root/backups`. Returns the backup
/// directory and the backup inside it.
async fn make_backup(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let (source, backups) = (root.join("source"), root.join("backups"));
    init::init(InitArgs {
        data_dir: source.clone(),
        admin_email: ADMIN.into(),
        admin_password: Some(PASSWORD.into()),
    })
    .await
    .unwrap();
    let (path, _) = backup::take(Source::Stopped(&source), &backups)
        .await
        .unwrap();
    (backups, path)
}

async fn post(addr: SocketAddr, path: &str, body: Value) -> wire::Reply {
    wire::request(addr, "POST", path, Some(body), None).await
}

/// Restores `backup` through the wizard into `data` and waits for setup mode to end.
async fn restore_through_the_wizard(data: &Path, backups: &Path, backup: &Path) {
    let server = start_with_backups(data, backups).await;
    let started = post(
        server.addr,
        "/api/setup/restore/start",
        json!({ "token": TOKEN, "path": backup }),
    )
    .await;
    assert_eq!(started.body["state"], "running", "{}", started.body);
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(120), server.run)
        .await
        .expect("restore finishes")
        .unwrap();
    assert!(matches!(outcome, Ok(Outcome::Initialized)));
}

#[tokio::test]
async fn the_wizard_lists_backups_with_their_manifest_and_reads_any_path() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    // Neighbours that are not backups stay out of the list.
    std::fs::create_dir(backups.join("holocron-backup-empty")).unwrap();
    std::fs::create_dir(backups.join("notes")).unwrap();
    let server = start_with_backups(&root.path().join("data"), &backups).await;

    let listed = post(
        server.addr,
        "/api/setup/restore/backups",
        json!({ "token": TOKEN }),
    )
    .await;
    assert_eq!(listed.status, 200, "{}", listed.body);
    let entries = listed.body["backups"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["path"], path.display().to_string());
    assert_eq!(entries[0]["release"], env!("CARGO_PKG_VERSION"));
    assert_eq!(entries[0]["upgrade"], false);
    assert!(entries[0]["bytes"].as_u64().unwrap() > 0);
    assert!(entries[0]["created_at_ms"].as_i64().unwrap() > 0);
    assert_eq!(listed.body["restore"]["state"], "idle");

    // Any other host path, from outside the backup directory.
    let moved = root.path().join("elsewhere");
    std::fs::rename(&path, &moved).unwrap();
    let inspected = post(
        server.addr,
        "/api/setup/restore/inspect",
        json!({ "token": TOKEN, "path": moved }),
    )
    .await;
    assert_eq!(
        inspected.body["backup"]["path"],
        moved.display().to_string()
    );
    let nothing = post(
        server.addr,
        "/api/setup/restore/inspect",
        json!({ "token": TOKEN, "path": root.path().join("none") }),
    )
    .await;
    assert_eq!(nothing.body["refusal"]["code"], "backup_invalid");
    server.stop().await;
}

#[tokio::test]
async fn a_restore_ends_setup_and_the_restored_admin_signs_in() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    let data = root.path().join("data");
    restore_through_the_wizard(&data, &backups, &path).await;

    init::check_initialized(&data).unwrap();
    assert!(!setup::needed(&data).unwrap());
    holocron_server::maintenance::versions::check_dir(&data)
        .await
        .unwrap();
    let db = Db::connect(&data).await.unwrap();
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE email = ?")
        .bind(ADMIN)
        .fetch_one(&db.reader)
        .await
        .unwrap();
    db.close().await;
    assert!(holocron_server::identity::verify_password(hash, PASSWORD.into()).await);
}

#[tokio::test]
async fn setup_modes_lock_file_does_not_block_a_restore() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(data.join("holocron.lock"), b"").unwrap();
    assert!(setup::needed(&data).unwrap());
    restore_through_the_wizard(&data, &backups, &path).await;
    init::check_initialized(&data).unwrap();
}

#[tokio::test]
async fn a_backup_from_a_newer_release_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    let manifest = path.join(MANIFEST_FILE);
    let mut text: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    text["versions"]["analytics"] = json!(text["versions"]["analytics"].as_i64().unwrap() + 1);
    std::fs::write(&manifest, text.to_string()).unwrap();
    let data = root.path().join("data");
    let server = start_with_backups(&data, &backups).await;

    let inspected = post(
        server.addr,
        "/api/setup/restore/inspect",
        json!({ "token": TOKEN, "path": path }),
    )
    .await;
    assert_eq!(inspected.body["refusal"]["code"], "data_newer_than_binary");
    let started = post(
        server.addr,
        "/api/setup/restore/start",
        json!({ "token": TOKEN, "path": path }),
    )
    .await;
    assert_eq!(started.body["state"], "failed");
    assert_eq!(started.body["error"]["code"], "data_newer_than_binary");
    assert!(!data.join("holocron.restore-in-progress").exists());
    server.stop().await;
}

#[tokio::test]
async fn restore_operations_need_the_token_and_end_with_setup() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    let server = start_with_backups(&root.path().join("data"), &backups).await;
    let addr = server.addr;
    let operations = [
        ("backups", json!({})),
        ("inspect", json!({ "path": path })),
        ("start", json!({ "path": path })),
    ];
    for (name, mut body) in operations.clone() {
        body["token"] = json!("not the token");
        let refused = post(addr, &format!("/api/setup/restore/{name}"), body).await;
        assert_eq!(
            (refused.status, refused.code()),
            (401, Some("setup_token_invalid")),
            "{name}"
        );
    }

    // An admin spends the token.
    import_admin(addr).await;
    for (name, mut body) in operations {
        body["token"] = json!(TOKEN);
        let spent = post(addr, &format!("/api/setup/restore/{name}"), body).await;
        assert_eq!(
            (spent.status, spent.code()),
            (503, Some("setup_required")),
            "{name}"
        );
    }
    server.stop().await;
}

#[tokio::test]
async fn an_interrupted_restore_starts_setup_mode_and_can_be_repeated() {
    let root = tempfile::tempdir().unwrap();
    let (backups, path) = make_backup(root.path()).await;
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::fs::write(data.join("holocron.restore-in-progress"), b"").unwrap();
    std::fs::write(data.join("control.sqlite"), b"half a copy").unwrap();
    assert!(setup::needed(&data).unwrap());

    let server = start_with_backups(&data, &backups).await;
    let status = wire::request(server.addr, "GET", "/api/setup/status", None, None).await;
    assert_eq!(status.body["restore_incomplete"], true);
    server.stop().await;

    restore_through_the_wizard(&data, &backups, &path).await;
    init::check_initialized(&data).unwrap();
}
