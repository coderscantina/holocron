//! Milestone 11 against real PostgreSQL 15 and MySQL 8.0: inspection of every fixture layout,
//! import of both supported snapshots through a read-only user, an unchanged source, resume after
//! killed runs, imported shares at their old URLs, and converted logins with 2FA.
//!
//! Ignored by default: `tools/import-rehearsal/test.sh` starts the databases of
//! `tools/import-rehearsal/compose.yaml`, sets `HOLOCRON_IMPORT_PG` and `HOLOCRON_IMPORT_MYSQL`
//! to their admin URLs and runs these tests.

mod common;

use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use common::{Answer, HOST, ORIGIN, test_budgets, wire};
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, Config};
use holocron_server::db::{Db, now_ms};
use holocron_server::identity::totp;
use holocron_server::imports::{self, ImportState, Options, RunArgs};
use holocron_server::init::{self, InitArgs};
use holocron_server::setup::{self, Outcome};
use holocron_server::{AppState, router};
use holocron_storage::{Dimensions, Event, EventKind, Lane, Record, Storage};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::{Connection, Executor, MySqlConnection, PgConnection, Row};
use tempfile::TempDir;
use tower::ServiceExt;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/import-schemas");

#[derive(Clone, Copy, PartialEq)]
enum Engine {
    Pg,
    My,
}

/// A fresh source database: its admin URL and the URL of a user that may only read it.
struct Source {
    engine: Engine,
    admin: String,
    read_only: String,
}

fn server(engine: Engine) -> String {
    let name = match engine {
        Engine::Pg => "HOLOCRON_IMPORT_PG",
        Engine::My => "HOLOCRON_IMPORT_MYSQL",
    };
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is not set; run tools/import-rehearsal/test.sh"))
}

/// `scheme://user:password@host:port/<database>` of `url` with another user and database.
fn with(url: &str, credentials: Option<&str>, database: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap();
    let (userinfo, host_path) = rest.split_once('@').unwrap();
    let host = host_path.split('/').next().unwrap();
    format!(
        "{scheme}://{}@{host}/{database}",
        credentials.unwrap_or(userinfo)
    )
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Loads `schema` (a captured dump), the `_prisma_migrations` rows of `migrations`, then each
/// of `data`, into a new database with a read-only user.
async fn source(engine: Engine, schema: &Path, migrations: &Path, data: &[String]) -> Source {
    let admin = server(engine);
    let database = unique("m11");
    let role = unique("ro");
    let rows: Vec<Value> = serde_json::from_str(&read(migrations)).unwrap();
    match engine {
        Engine::Pg => {
            let mut conn = PgConnection::connect(&admin).await.unwrap();
            conn.execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
                .await
                .unwrap();
            conn.close().await.unwrap();
            let mut conn = PgConnection::connect(&with(&admin, None, &database))
                .await
                .unwrap();
            sqlx::raw_sql(sqlx::AssertSqlSafe(read(schema)))
                .execute(&mut conn)
                .await
                .unwrap();
            conn.execute("SET search_path TO public").await.unwrap();
            for m in rows {
                sqlx::query(
                    "INSERT INTO _prisma_migrations (id, checksum, finished_at, migration_name, logs,
                         rolled_back_at, started_at, applied_steps_count)
                     VALUES (gen_random_uuid()::text, $1, CASE WHEN $2 THEN now() END, $3, $4,
                         CASE WHEN $5 THEN now() END, now(), $6)",
                )
                .bind(m["checksum"].as_str())
                .bind(!m["finished_at"].is_null())
                .bind(m["migration_name"].as_str())
                .bind(m["logs"].as_str())
                .bind(!m["rolled_back_at"].is_null())
                .bind(m["applied_steps_count"].as_i64().unwrap() as i32)
                .execute(&mut conn)
                .await
                .unwrap();
            }
            for sql in data {
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql.clone()))
                    .execute(&mut conn)
                    .await
                    .unwrap();
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "CREATE ROLE {role} LOGIN PASSWORD 'ro';
                 GRANT CONNECT ON DATABASE {database} TO {role};
                 GRANT USAGE ON SCHEMA public TO {role};
                 GRANT SELECT ON ALL TABLES IN SCHEMA public TO {role};"
            )))
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        Engine::My => {
            let mut conn = MySqlConnection::connect(&admin).await.unwrap();
            conn.execute(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
                .await
                .unwrap();
            conn.execute(sqlx::AssertSqlSafe(format!(
                "CREATE USER '{role}'@'%' IDENTIFIED BY 'ro'"
            )))
            .await
            .unwrap();
            conn.execute(sqlx::AssertSqlSafe(format!(
                "GRANT SELECT ON {database}.* TO '{role}'@'%'"
            )))
            .await
            .unwrap();
            conn.close().await.unwrap();
            let mut conn = MySqlConnection::connect(&with(&admin, None, &database))
                .await
                .unwrap();
            sqlx::raw_sql(sqlx::AssertSqlSafe(read(schema)))
                .execute(&mut conn)
                .await
                .unwrap();
            for m in rows {
                sqlx::query(
                    "INSERT INTO _prisma_migrations (id, checksum, finished_at, migration_name, logs,
                         rolled_back_at, started_at, applied_steps_count)
                     VALUES (UUID(), ?, IF(?, NOW(3), NULL), ?, ?, IF(?, NOW(3), NULL), NOW(3), ?)",
                )
                .bind(m["checksum"].as_str())
                .bind(!m["finished_at"].is_null())
                .bind(m["migration_name"].as_str())
                .bind(m["logs"].as_str())
                .bind(!m["rolled_back_at"].is_null())
                .bind(m["applied_steps_count"].as_i64().unwrap())
                .execute(&mut conn)
                .await
                .unwrap();
            }
            for sql in data {
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql.clone()))
                    .execute(&mut conn)
                    .await
                    .unwrap();
            }
            conn.close().await.unwrap();
        }
    }
    Source {
        engine,
        read_only: with(&admin, Some(&format!("{role}:ro")), &database),
        admin: with(&admin, None, &database),
    }
}

/// A supported snapshot with its sample rows and any `extra` SQL.
async fn snapshot(engine: Engine, extra: &[String]) -> Source {
    let dir = Path::new(FIXTURES).join(match engine {
        Engine::Pg => "postgresql-3.4.0",
        Engine::My => "mysql-2.20.2",
    });
    let mut data = vec![read(&dir.join("sample-data.sql"))];
    data.extend_from_slice(extra);
    source(
        engine,
        &dir.join("schema.sql"),
        &dir.join("migrations.json"),
        &data,
    )
    .await
}

/// A content checksum of every table, through the admin connection.
async fn checksums(source: &Source) -> BTreeMap<String, String> {
    match source.engine {
        Engine::Pg => {
            let mut conn = PgConnection::connect(&source.admin).await.unwrap();
            let tables: Vec<String> = sqlx::query_scalar(
                "SELECT tablename::text FROM pg_tables WHERE schemaname = 'public' ORDER BY 1",
            )
            .fetch_all(&mut conn)
            .await
            .unwrap();
            let mut sums = BTreeMap::new();
            for table in tables {
                let sum: String = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) || ':' || md5(coalesce(string_agg(t::text, '|' ORDER BY t::text), ''))
                     FROM public.\"{table}\" t"
                )))
                .fetch_one(&mut conn)
                .await
                .unwrap();
                sums.insert(table, sum);
            }
            sums
        }
        Engine::My => {
            let mut conn = MySqlConnection::connect(&source.admin).await.unwrap();
            let tables: Vec<String> = sqlx::query_scalar(
                "SELECT CAST(table_name AS CHAR) FROM information_schema.tables WHERE table_schema = DATABASE()",
            )
            .fetch_all(&mut conn)
            .await
            .unwrap();
            let mut sums = BTreeMap::new();
            for table in tables {
                let row = sqlx::query(sqlx::AssertSqlSafe(format!("CHECKSUM TABLE `{table}`")))
                    .fetch_one(&mut conn)
                    .await
                    .unwrap();
                let sum: Option<i64> = row.try_get(1).unwrap();
                sums.insert(table, format!("{sum:?}"));
            }
            sums
        }
    }
}

/// An initialized data directory with an admin who is not from the source.
async fn data_dir() -> TempDir {
    let dir = TempDir::new().unwrap();
    init::init(InitArgs {
        data_dir: dir.path().to_path_buf(),
        admin_email: "ops@example.test".into(),
        admin_password: Some("operator password 1".into()),
    })
    .await
    .unwrap();
    dir
}

fn args(dir: &Path, source: &str, key: Option<PathBuf>) -> RunArgs {
    RunArgs {
        data_dir: dir.to_path_buf(),
        source: source.into(),
        options: Options {
            timezone: "Europe/Vienna".into(),
            login_domain: "umami.invalid".into(),
            chunk_rows: 2,
            retention: holocron_protocol::retention::Retention::OneYear,
        },
        two_factor_key_file: key,
        disk_reserve_mib: 0,
    }
}

fn instant(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .unwrap()
        .timestamp_millis()
}

/// The router over an imported data directory, with a web app to answer client routes.
struct App {
    router: Router,
    _web: TempDir,
}

impl App {
    async fn open(dir: &Path) -> Self {
        let web = TempDir::new().unwrap();
        std::fs::write(
            web.path().join("index.html"),
            "<!doctype html><div id=app></div>",
        )
        .unwrap();
        let db = Db::open(dir).await.unwrap();
        let config = Config {
            data_dir: dir.to_path_buf(),
            bind: "127.0.0.1:0".parse().unwrap(),
            web_dir: Some(web.path().to_path_buf()),
            public_origin: None,
            instance_name: "Import test".into(),
            setup_token: None,
            collect: CollectConfig::default(),
            disk_reserve_bytes: 0,
        };
        let storage = Arc::new(Storage::open(dir, test_budgets()).await.unwrap());
        let state = AppState::new(db, storage, config, Clock::system())
            .await
            .unwrap();
        Self {
            router: router(state),
            _web: web,
        }
    }

    async fn send(&self, request: Request<Body>) -> Answer {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        Answer {
            status,
            headers,
            bytes,
        }
    }

    async fn get(&self, path: &str, authorization: Option<&str>) -> Answer {
        let mut request = Request::get(path).header(header::HOST, HOST);
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    async fn command(&self, cookie: Option<&str>, name: &str, mut body: Value) -> Answer {
        body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
        let mut request = Request::post(format!("/api/commands/{name}"))
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header("x-holocron-request", "1")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }
}

fn cookie(answer: &Answer, name: &str) -> Option<String> {
    answer
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && pair.len() > name.len() + 1)
        .map(str::to_owned)
}

async fn control(dir: &Path, sql: &str) -> Vec<Vec<String>> {
    let db = Db::open(dir).await.unwrap();
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_all(&db.reader)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..r.len())
                .map(|i| {
                    r.try_get::<Option<String>, _>(i)
                        .map(Option::unwrap_or_default)
                        .or_else(|_| r.try_get::<i64, _>(i).map(|v| v.to_string()))
                        .unwrap()
                })
                .collect()
        })
        .collect();
    db.close().await;
    rows
}

#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn every_fixture_layout_inspects_like_its_expected_outcome_on_a_real_database() {
    let refusal = Path::new(FIXTURES).join("refusal");
    let mut cases = 0;
    for entry in std::fs::read_dir(&refusal).unwrap() {
        let dir = entry.unwrap().path();
        let expected: Value = serde_json::from_str(&read(&dir.join("expected.json"))).unwrap();
        let engine = if expected["engine"] == "postgresql" {
            Engine::Pg
        } else {
            Engine::My
        };
        // Case g has no DDL: upstream 3.x has no MySQL schema. Its migration rows go into the
        // 2.20.2 schema.
        let schema = match dir.join("schema.sql") {
            path if path.exists() => path,
            _ => Path::new(FIXTURES).join("mysql-2.20.2/schema.sql"),
        };
        let source = source(engine, &schema, &dir.join("migrations.json"), &[]).await;
        let (report, outcome) = imports::inspect(&source.read_only, chrono_tz::UTC, now_ms())
            .await
            .unwrap();
        let mut details = expected["details"].clone();
        details.as_object_mut().unwrap().remove("note");
        assert_eq!(report["outcome"], "refuse", "{}", dir.display());
        assert_eq!(
            report["code"],
            expected["code"],
            "{}: {report}",
            dir.display()
        );
        assert_eq!(report["details"], details, "{}", dir.display());
        assert_eq!(outcome.unwrap_err().exit as u8, 3);
        cases += 1;
    }
    assert_eq!(cases, 10);

    for engine in [Engine::Pg, Engine::My] {
        let source = snapshot(engine, &[]).await;
        let (report, outcome) = imports::inspect(&source.read_only, chrono_tz::UTC, now_ms())
            .await
            .unwrap();
        assert_eq!(report["outcome"], "accept", "{report}");
        assert_eq!(report["credentials"], "read_only");
        assert_eq!(report["warnings"], json!([]));
        assert!(outcome.is_ok());
        // Credentials that could write get a warning, and the import reads through them.
        let before = checksums(&source).await;
        let (report, outcome) = imports::inspect(&source.admin, chrono_tz::UTC, now_ms())
            .await
            .unwrap();
        assert_eq!(report["outcome"], "accept");
        assert_eq!(report["warnings"][0]["code"], "credentials_writable");
        assert!(outcome.is_ok());
        let dir = data_dir().await;
        let manifest = imports::run(
            args(dir.path(), &source.admin, None),
            test_budgets(),
            now_ms(),
        )
        .await
        .unwrap();
        assert_eq!(manifest["status"], "reconciled", "{manifest:#}");
        assert_eq!(checksums(&source).await, before, "the source changed");
    }
}

/// 0021: a URL that asks for TLS is read over TLS. MySQL 8.0 serves TLS with the certificate it
/// generates, and `REQUIRED` fails without it. PostgreSQL shares sqlx's rustls connector.
#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn a_source_url_that_asks_for_tls_is_read_over_tls() {
    let source = snapshot(Engine::My, &[]).await;
    let url = format!("{}?ssl-mode=REQUIRED", source.read_only);
    let (report, outcome) = imports::inspect(&url, chrono_tz::UTC, now_ms())
        .await
        .unwrap();
    assert_eq!(report["outcome"], "accept", "{report}");
    assert!(outcome.is_ok());
}

#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn both_snapshots_import_reconciled_through_a_read_only_user_and_leave_the_source_unchanged()
{
    for (engine, clock, sites, extra) in [
        (Engine::Pg, "2026-03-01T00:00:00Z", 2, vec![saved_replay()]),
        (Engine::My, "2024-03-01T00:00:00Z", 2, vec![]),
    ] {
        let source = snapshot(engine, &extra).await;
        let before = checksums(&source).await;
        let dir = data_dir().await;
        let manifest = imports::run(
            args(dir.path(), &source.read_only, None),
            test_budgets(),
            instant(clock),
        )
        .await
        .unwrap();
        assert_eq!(checksums(&source).await, before, "the source changed");
        assert_eq!(manifest["status"], "reconciled", "{manifest:#}");
        assert_eq!(
            manifest["sites"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["kind"] == "site")
                .count(),
            sites
        );

        // Website IDs are the source UUIDs; revenue comes through in minor units.
        let site = manifest["sites"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "00000000-0000-4000-8000-000000000201")
            .unwrap();
        assert_eq!(site["events"]["pageview"]["destination"], 1);
        assert_eq!(site["events"]["custom_event"]["destination"], 1);
        assert_eq!(site["revenue"][0]["destination_minor"], 4990);
        assert_eq!(site["sessions"], 1);
        let users = control(dir.path(), "SELECT email FROM users ORDER BY email").await;
        assert_eq!(
            users,
            [
                ["admin@umami.invalid"],
                ["member@umami.invalid"],
                ["ops@example.test"]
            ]
        );
        assert!(
            manifest["inventory"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["reason"] == "member_loses_site_delete")
        );
        let ImportState::Complete { .. } = imports::state(&Db::open(dir.path()).await.unwrap())
            .await
            .unwrap()
        else {
            panic!("import not complete");
        };
        if engine == Engine::Pg {
            // The sample replay is not gzip and stays in the inventory; the added one is stored
            // and saved. Its heatmap click arrives too.
            let team = manifest["sites"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == "00000000-0000-4000-8000-000000000202")
                .unwrap();
            assert_eq!(
                team["replay_parts"],
                json!({ "source": 2, "destination": 1, "inventoried": 1 })
            );
            assert_eq!(team["heatmaps"], json!({ "source": 1, "destination": 1 }));
            let captures = rows_of(
                &dir.path().join("recordings.sqlite"),
                "SELECT count(*) FROM captures WHERE saved_at IS NOT NULL",
            )
            .await;
            assert_eq!(captures, 1);
        }
    }
}

/// A gzip-compressed rrweb part as Umami 3.4.0 stores replays, saved, inside replay retention.
fn saved_replay() -> String {
    let events = r#"[{"type":4,"data":{"href":"https://team.example.test/docs","width":1280,"height":720},"timestamp":1772013600000},{"type":3,"data":{"source":1},"timestamp":1772013601000}]"#;
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(events.as_bytes()).unwrap();
    let hex = data_encoding::HEXLOWER.encode(&gzip.finish().unwrap());
    format!(
        "insert into session_replay (replay_id, website_id, session_id, visit_id, chunk_index, events, event_count, started_at, ended_at, created_at) values
           ('00000000-0000-4000-8000-000000000802', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000302',
            '00000000-0000-4000-8000-000000000a03', 0, decode('{hex}', 'hex'), 2, '2026-02-25 10:00:00+00', '2026-02-25 10:00:01+00', '2026-02-25 10:00:02+00');
         insert into session_replay_saved (saved_replay_id, name, website_id, visit_id, created_at, updated_at) values
           ('00000000-0000-4000-8000-000000000812', 'Docs visit', '00000000-0000-4000-8000-000000000202',
            '00000000-0000-4000-8000-000000000a03', '2026-02-26 08:00:00+00', '2026-02-26 08:00:00+00');"
    )
}

async fn rows_of(path: &Path, sql: &str) -> i64 {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(path)
        .read_only(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap();
    let count = sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
        .fetch_one(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    count
}

#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn imported_public_shares_answer_at_their_old_urls_with_historical_limitations() {
    // 3.x: `share.slug`, opened at /share/<slug>/<section>.
    let pg = snapshot(Engine::Pg, &[]).await;
    let dir = data_dir().await;
    imports::run(
        args(dir.path(), &pg.read_only, None),
        test_budgets(),
        instant("2026-03-01T00:00:00Z"),
    )
    .await
    .unwrap();
    let app = App::open(dir.path()).await;
    let resolved = app
        .get("/api/queries/sharing.resolve", Some("Share sample-share"))
        .await;
    assert_eq!(resolved.status, 200, "{}", resolved.json());
    let state = resolved.json()["target"]["state"].clone();
    assert_eq!(state["site"], "00000000-0000-4000-8000-000000000201");
    assert_eq!(
        app.get("/share/sample-share/overview", None).await.status,
        200
    );

    // Results that reach imported history say so.
    let mut state = state;
    state["range"] = json!({ "kind": "days", "start": "2026-01-15", "end": "2026-02-15" });
    let query = format!(
        "/api/queries/analytics.explore?state={}",
        url::form_urlencoded::byte_serialize(state.to_string().as_bytes()).collect::<String>()
    );
    let explored = app.get(&query, Some("Share sample-share")).await;
    assert_eq!(explored.status, 200, "{}", explored.json());
    let result = explored.json();
    assert_eq!(
        result["freshness"]["imported"]["source"],
        "umami-3.4.0-postgresql"
    );
    assert_eq!(result["main"]["totals"][0]["v"], 1, "{result:#}");

    // 2.x: `website.share_id`, opened at /share/<id>/<website name>. Imported shares expire 366
    // days after the import clock, so this import runs today.
    let my = snapshot(Engine::My, &[]).await;
    let dir = data_dir().await;
    imports::run(
        args(dir.path(), &my.read_only, None),
        test_budgets(),
        now_ms(),
    )
    .await
    .unwrap();
    let app = App::open(dir.path()).await;
    assert_eq!(
        app.get("/api/queries/sharing.resolve", Some("Share sample-share"))
            .await
            .status,
        200
    );
    assert_eq!(
        app.get("/share/sample-share/example.test", None)
            .await
            .status,
        200
    );
    assert_eq!(
        app.get("/api/queries/sharing.resolve", Some("Share unknown-slug"))
            .await
            .status,
        404
    );
}

#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn converted_accounts_sign_in_with_their_umami_password_and_explicit_2fa() {
    // Two accounts as Umami 3.4.0 writes them: bcrypt passwords, and a TOTP secret encrypted with
    // the installation's TWO_FACTOR_ENCRYPTION_KEY.
    let key = [42u8; 32];
    let secret = b"12345678901234567890";
    let iv = [3u8; 12];
    let sealed = Aes256Gcm::new_from_slice(&key)
        .unwrap()
        .encrypt(&Nonce::from(iv), totp::base32(secret).as_bytes())
        .unwrap();
    let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
    let hex = data_encoding::HEXLOWER;
    let stored = format!(
        "{}:{}:{}",
        hex.encode(ciphertext),
        hex.encode(&iv),
        hex.encode(tag)
    );
    let hash = bcrypt::hash("umami password 1", 4).unwrap();
    let extra = format!(
        "insert into \"user\" (user_id, username, password, role, created_at) values
           ('00000000-0000-4000-8000-000000000103', 'with-2fa', '{hash}', 'user', now()),
           ('00000000-0000-4000-8000-000000000104', 'Plain@Example.org', '{hash}', 'user', now());
         insert into two_factor_auth (id, user_id, secret, is_enabled, created_at, updated_at) values
           ('tfa-real', '00000000-0000-4000-8000-000000000103', '{stored}', true, now(), now());"
    );
    let pg = snapshot(Engine::Pg, &[extra]).await;
    let dir = data_dir().await;
    let key_file = dir.path().join("umami-2fa.key");
    std::fs::File::create(&key_file)
        .unwrap()
        .write_all(hex.encode(&key).as_bytes())
        .unwrap();
    let manifest = imports::run(
        args(dir.path(), &pg.read_only, Some(key_file)),
        test_budgets(),
        instant("2026-03-01T00:00:00Z"),
    )
    .await
    .unwrap();
    let logins: BTreeMap<String, Value> = manifest["logins"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["email"].as_str().unwrap().to_owned(), l.clone()))
        .collect();
    assert_eq!(logins["with-2fa@umami.invalid"]["two_factor"], "kept");
    assert_eq!(logins["plain@example.org"]["two_factor"], "none");
    // The sample secret does not decrypt, so that account must enroll again.
    assert_eq!(
        logins["member@umami.invalid"]["two_factor"],
        "enrollment_required"
    );

    let app = App::open(dir.path()).await;
    let login = |email: &str| json!({ "email": email, "password": "umami password 1" });
    let plain = app
        .command(None, "auth.login", login("plain@example.org"))
        .await;
    assert_eq!(plain.json()["status"], "signed_in", "{}", plain.json());
    let hashes = control(
        dir.path(),
        "SELECT password_hash FROM users WHERE email = 'plain@example.org'",
    )
    .await;
    assert!(
        hashes[0][0].starts_with("$argon2"),
        "the bcrypt hash is replaced at first login"
    );
    let again = app
        .command(None, "auth.login", login("plain@example.org"))
        .await;
    assert_eq!(again.json()["status"], "signed_in");
    let wrong = app
        .command(
            None,
            "auth.login",
            json!({ "email": "plain@example.org", "password": "wrong password!" }),
        )
        .await;
    assert_eq!(wrong.code(), "invalid_credentials");

    let password = app
        .command(None, "auth.login", login("with-2fa@umami.invalid"))
        .await;
    assert_eq!(password.json()["status"], "second_factor_required");
    let challenge = cookie(&password, "holocron_challenge").unwrap();
    let code = totp::code(secret, totp::step(now_ms()));
    let verified = app
        .command(
            Some(&challenge),
            "auth.verifySecondFactor",
            json!({ "kind": "totp", "code": code }),
        )
        .await;
    assert_eq!(
        verified.json()["status"],
        "signed_in",
        "{}",
        verified.json()
    );
    // No login session or API key came from the source.
    assert_eq!(
        control(dir.path(), "SELECT count(*) FROM api_keys").await,
        [["0"]]
    );
}

/// Sessions, events and properties spread over the last 50 days, for the resume test.
fn generated_traffic() -> String {
    let site = "00000000-0000-4000-8000-000000000201";
    format!(
        "insert into session (session_id, website_id, browser, os, device, country, created_at)
           select md5('s' || i)::uuid, '{site}', 'chrome', 'Mac OS', 'desktop', 'AT', now() - (i % 50) * interval '1 day'
           from generate_series(0, 399) i;
         insert into website_event (event_id, website_id, session_id, visit_id, created_at, url_path, event_type, event_name, hostname)
           select md5('e' || i)::uuid, '{site}', md5('s' || (i % 400))::uuid, md5('v' || (i % 400))::uuid,
                  now() - (i % 400 % 50) * interval '1 day' + (i % 9) * interval '4 minutes',
                  '/p/' || (i % 20), case when i % 5 = 0 then 2 else 1 end,
                  case when i % 5 = 0 then 'signup' end, 'example.test'
           from generate_series(0, 5999) i;
         insert into event_data (event_data_id, website_id, website_event_id, data_key, string_value, data_type, created_at)
           select md5('d' || i)::uuid, '{site}', md5('e' || i)::uuid, 'plan', 'pro', 1, now()
           from generate_series(0, 5999, 5) i;
         insert into session_data (session_data_id, website_id, session_id, data_key, string_value, data_type, created_at)
           select md5('sd' || i)::uuid, '{site}', md5('s' || i)::uuid, 'tier', 'gold', 1, now() - (i % 50) * interval '1 day'
           from generate_series(0, 399) i;"
    )
}

/// Total committed chunks of a running import, read beside the writer.
async fn chunks(dir: &Path) -> i64 {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.join("control.sqlite"))
        .read_only(true);
    let Ok(mut conn) = sqlx::SqliteConnection::connect_with(&options).await else {
        return 0;
    };
    let total = sqlx::query_scalar::<_, i64>("SELECT coalesce(sum(chunks), 0) FROM import_steps")
        .fetch_one(&mut conn)
        .await
        .unwrap_or(0);
    let _ = conn.close().await;
    total
}

fn holocron(args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_holocron"));
    command
        .args(args)
        .env("HOLOCRON_LOG", "warn")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

fn run_command<'a>(dir: &'a str, source: &'a str) -> Vec<&'a str> {
    vec![
        "import",
        "run",
        "--data-dir",
        dir,
        "--source",
        source,
        "--timezone",
        "Europe/Vienna",
        "--chunk-rows",
        "150",
        "--disk-reserve-mib",
        "0",
    ]
}

/// The parts of a manifest a resumed import must reproduce.
fn reconciled(manifest: &Value) -> Value {
    json!({
        "status": manifest["status"], "digests": manifest["digests"], "sites": manifest["sites"],
        "configuration": manifest["configuration"], "inventory": manifest["inventory"],
        "samples": manifest["samples"], "steps": manifest["steps"],
    })
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()));
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}

#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn killed_imports_resume_to_the_same_destination_without_duplicates() {
    let source = snapshot(Engine::Pg, &[generated_traffic()]).await;
    // Both directories share one installation secret, so their pseudonyms can be compared.
    let base = data_dir().await;
    let work = TempDir::new().unwrap();
    let (clean, killed) = (work.path().join("clean"), work.path().join("killed"));
    copy_dir(base.path(), &clean);
    copy_dir(base.path(), &killed);

    let output = holocron(&run_command(clean.to_str().unwrap(), &source.read_only))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected["status"], "reconciled", "{expected:#}");

    let mut kills = Vec::new();
    let manifest = loop {
        let target = chunks(&killed).await + 7;
        let mut child = holocron(&run_command(killed.to_str().unwrap(), &source.read_only))
            .spawn()
            .unwrap();
        let finished = loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "a resumed import failed");
                break true;
            }
            if chunks(&killed).await >= target {
                child.kill().unwrap();
                child.wait().unwrap();
                kills.push(chunks(&killed).await);
                // `serve` on an interrupted import starts in setup mode to resume it (0021).
                if kills.len() == 1 {
                    let (mut server, addr) = serve(&killed).await;
                    let config = wire::request(addr, "GET", "/config.json", None, None).await;
                    assert_eq!(config.body["setup"], true);
                    server.kill().unwrap();
                    server.wait().unwrap();
                }
                break false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        if finished {
            let status = holocron(&["import", "status", "--data-dir", killed.to_str().unwrap()])
                .output()
                .unwrap();
            let report: Value = serde_json::from_slice(&status.stdout).unwrap();
            assert_eq!(report["import"]["state"], "complete");
            break report["manifest"].clone();
        }
        assert!(kills.len() < 100, "the import never finished");
    };
    eprintln!("killed at committed chunk counts {kills:?}, then resumed to the end");
    assert!(
        kills.len() >= 3,
        "only {} interruptions; the data set is too small to test resume",
        kills.len()
    );
    assert_eq!(reconciled(&manifest), reconciled(&expected));
    let events = &manifest["sites"][0]["events"];
    let imported = events["pageview"]["destination"].as_i64().unwrap()
        + events["custom_event"]["destination"].as_i64().unwrap();
    assert!(imported >= 6000, "{imported} events");

    // A retry of a finished import changes nothing and answers the same manifest.
    let retry = holocron(&run_command(killed.to_str().unwrap(), &source.read_only))
        .output()
        .unwrap();
    assert!(retry.status.success());
    let again: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(reconciled(&again), reconciled(&expected));
}

/// `holocron <command>` against `dir`: the exit code and stderr.
fn operator_job(command: &[&str], dir: &Path) -> (Option<i32>, String) {
    let mut line = command.to_vec();
    line.extend(["--data-dir", dir.to_str().unwrap()]);
    let output = holocron(&line).output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// `holocron serve` on `dir` at a free port, once it answers.
async fn serve(dir: &Path) -> (std::process::Child, SocketAddr) {
    let addr: SocketAddr = format!("127.0.0.1:{}", wire::free_port()).parse().unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_holocron"))
        .args(["serve", "--data-dir", dir.to_str().unwrap(), "--bind"])
        .arg(addr.to_string())
        .args(["--disk-reserve-mib", "0"])
        .env("HOLOCRON_LOG", "warn")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wire::ready(addr).await;
    (child, addr)
}

/// Committed chunks of the events step while it is not done.
async fn events_chunks(dir: &Path) -> i64 {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(dir.join("control.sqlite"))
        .read_only(true);
    let Ok(mut conn) = sqlx::SqliteConnection::connect_with(&options).await else {
        return 0;
    };
    let chunks = sqlx::query_scalar::<_, i64>(
        "SELECT chunks FROM import_steps WHERE step = 'events' AND NOT done",
    )
    .fetch_optional(&mut conn)
    .await
    .ok()
    .flatten()
    .unwrap_or(0);
    let _ = conn.close().await;
    chunks
}

/// The wizard's options for the import `args` and `run_command` describe.
fn wizard_options(chunk_rows: i64, retention: &str) -> Value {
    json!({ "timezone": "Europe/Vienna", "login_domain": "umami.invalid",
            "chunk_rows": chunk_rows, "retention": retention })
}

/// Signs the `data_dir` admin in through setup mode and answers the cookie.
async fn wizard_sign_in(addr: SocketAddr) -> String {
    let credentials = json!({ "email": "ops@example.test", "password": "operator password 1" });
    let signed_in =
        wire::request(addr, "POST", "/api/setup/sign-in", Some(credentials), None).await;
    signed_in.session().expect("signed in")
}

/// The wizard's endpoints import both supported snapshots into a directory the admin step
/// left behind, with the CLI's manifest and digests, while collection waits (0021).
#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn both_snapshots_import_through_the_wizard_with_the_digests_of_the_cli() {
    for (engine, clock) in [
        (Engine::Pg, "2026-03-01T00:00:00Z"),
        (Engine::My, "2024-03-01T00:00:00Z"),
    ] {
        let source = snapshot(engine, &[]).await;
        // Both directories share one installation secret, so their pseudonyms can be compared.
        let base = data_dir().await;
        let work = TempDir::new().unwrap();
        let (cli, wizard) = (work.path().join("cli"), work.path().join("wizard"));
        copy_dir(base.path(), &cli);
        copy_dir(base.path(), &wizard);
        let expected = imports::run(
            args(&cli, &source.read_only, None),
            test_budgets(),
            instant(clock),
        )
        .await
        .unwrap();

        // The admin step of the import path, then a restart.
        std::fs::write(wizard.join(init::SETUP_IMPORT_MARKER), b"").unwrap();
        let config = Config {
            data_dir: wizard.clone(),
            bind: "127.0.0.1:0".parse().unwrap(),
            web_dir: None,
            public_origin: None,
            instance_name: "Import test".into(),
            setup_token: None,
            collect: CollectConfig::default(),
            disk_reserve_bytes: 0,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (_stop, stopped) = tokio::sync::watch::channel(false);
        let run = tokio::spawn(setup::run(
            config,
            Clock::frozen(instant(clock)),
            listener,
            stopped,
        ));
        wire::ready(addr).await;
        let cookie = wizard_sign_in(addr).await;

        let inspect = json!({ "source": source.read_only, "timezone": "Europe/Vienna" });
        let inspected = wire::request(
            addr,
            "POST",
            "/api/setup/import/inspect",
            Some(inspect),
            Some(&cookie),
        )
        .await;
        let preview = &inspected.body["preview"];
        assert!(
            preview["events"].as_i64().unwrap() > 0,
            "{}",
            inspected.body
        );
        assert_eq!(preview["retention"].as_array().unwrap().len(), 6);

        let start = json!({ "source": source.read_only, "options": wizard_options(2, "1y") });
        let started = wire::request(
            addr,
            "POST",
            "/api/setup/import/run",
            Some(start),
            Some(&cookie),
        )
        .await;
        assert_eq!(started.body["state"], "running", "{}", started.body);
        let collect = wire::request(addr, "POST", "/api/send", Some(json!({})), None).await;
        assert_eq!(collect.status, 503);
        let done = wire::event_until(addr, "/api/setup/import/progress", &cookie, |frame| {
            frame["state"] != "running"
        })
        .await;
        assert_eq!(done["state"], "complete", "{done}");
        assert!(
            done["steps"]
                .as_array()
                .unwrap()
                .iter()
                .all(|step| step["state"] == "done")
        );

        let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
        assert_eq!(status.body["import"]["report"]["reconciled"], true);
        let manifest = wire::request(
            addr,
            "GET",
            "/api/setup/import/manifest",
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(reconciled(&manifest.body), reconciled(&expected));

        let finish = wire::request(
            addr,
            "POST",
            "/api/setup/finish",
            Some(json!({})),
            Some(&cookie),
        )
        .await;
        assert_eq!(finish.status, 200, "{}", finish.body);
        assert!(matches!(run.await.unwrap(), Ok(Outcome::Initialized)));
        assert!(!setup::needed(&wizard).unwrap());
    }
}

/// A server killed inside the events step comes back in setup mode with the import interrupted,
/// resumes from the wizard with the source given again, ends with the digests of an
/// uninterrupted CLI run, and serves normally once the report is acknowledged.
#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn a_server_killed_during_the_events_step_resumes_from_the_wizard_to_the_same_digests() {
    let source = snapshot(Engine::Pg, &[generated_traffic()]).await;
    let base = data_dir().await;
    let work = TempDir::new().unwrap();
    let (clean, killed) = (work.path().join("clean"), work.path().join("killed"));
    copy_dir(base.path(), &clean);
    copy_dir(base.path(), &killed);
    let output = holocron(&run_command(clean.to_str().unwrap(), &source.read_only))
        .output()
        .unwrap();
    assert!(output.status.success());
    let expected: Value = serde_json::from_slice(&output.stdout).unwrap();

    std::fs::write(killed.join(init::SETUP_IMPORT_MARKER), b"").unwrap();
    let (mut server, addr) = serve(&killed).await;
    let cookie = wizard_sign_in(addr).await;
    let start = json!({ "source": source.read_only, "options": wizard_options(150, "forever") });
    let started = wire::request(
        addr,
        "POST",
        "/api/setup/import/run",
        Some(start.clone()),
        Some(&cookie),
    )
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    while events_chunks(&killed).await < 3 {
        assert!(
            server.try_wait().unwrap().is_none(),
            "the server stopped before it could be killed"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    server.kill().unwrap();
    server.wait().unwrap();
    assert!(
        events_chunks(&killed).await >= 3,
        "killed inside the events step"
    );

    let (mut server, addr) = serve(&killed).await;
    let collect = wire::request(addr, "POST", "/api/send", Some(json!({})), None).await;
    assert_eq!(collect.status, 503);
    let status = wire::request(addr, "GET", "/api/setup/status", None, Some(&cookie)).await;
    assert_eq!(
        status.body["import"]["state"], "interrupted",
        "{}",
        status.body
    );
    assert_eq!(status.body["import"]["options"]["chunk_rows"], 150);
    let resumed = wire::request(
        addr,
        "POST",
        "/api/setup/import/run",
        Some(start),
        Some(&cookie),
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let done = wire::event_until(addr, "/api/setup/import/progress", &cookie, |frame| {
        frame["state"] != "running"
    })
    .await;
    assert_eq!(done["state"], "complete", "{done}");
    let manifest = wire::request(
        addr,
        "GET",
        "/api/setup/import/manifest",
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(reconciled(&manifest.body), reconciled(&expected));

    let finish = wire::request(
        addr,
        "POST",
        "/api/setup/finish",
        Some(json!({})),
        Some(&cookie),
    )
    .await;
    assert_eq!(finish.status, 200, "{}", finish.body);
    // The same process and port serve normally now.
    let mut setup_mode = true;
    for _ in 0..600 {
        if let Ok(config) = wire::send(addr, "GET", "/config.json", None, None).await
            && config.body["setup"] == false
        {
            setup_mode = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!setup_mode, "serve never left setup mode");
    assert!(server.try_wait().unwrap().is_none());
    server.kill().unwrap();
    server.wait().unwrap();
}

/// 0014: while an import is unfinished, operator jobs refuse with `import_in_progress` instead of
/// waiting for it; erasing or expiring rows the resumed import writes again would undo them.
#[tokio::test]
#[ignore = "needs the databases of tools/import-rehearsal/test.sh"]
async fn a_killed_import_refuses_erasure_and_retention_until_it_resumes() {
    let source = snapshot(Engine::Pg, &[generated_traffic()]).await;
    let dir = data_dir().await;
    let path = dir.path();
    let site = "00000000-0000-4000-8000-000000000201";
    let erase = |dir: &Path| {
        operator_job(
            &["erase-profile", "--site", site, "--distinct-id", "visitor"],
            dir,
        )
    };

    let mut child = holocron(&run_command(path.to_str().unwrap(), &source.read_only))
        .spawn()
        .unwrap();
    while chunks(path).await < 3 {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the import finished before it could be interrupted"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    child.kill().unwrap();
    child.wait().unwrap();

    for (exit, error) in [erase(path), operator_job(&["retention"], path)] {
        assert_eq!(exit, Some(3), "{error}");
        assert!(error.starts_with("error: import_in_progress:"), "{error}");
    }

    let resumed = holocron(&run_command(path.to_str().unwrap(), &source.read_only))
        .output()
        .unwrap();
    assert!(resumed.status.success(), "the import did not resume");
    assert_eq!(erase(path).0, Some(0));
    assert_eq!(operator_job(&["retention"], path).0, Some(0));
}

fn imported_event(id: u128) -> Record {
    Record::Event(Event {
        id,
        site: 7,
        kind: EventKind::Pageview,
        event_time_ms: 1_790_812_800_000,
        receive_time_ms: 1_790_812_800_000,
        local_day: 20_727,
        daily_id: Some(id),
        profile_id: None,
        client_event_id: None,
        dimensions: Dimensions::default(),
        performance: None,
        revenue: None,
        properties: Vec::new(),
    })
}

/// The window the kill test hits only by chance: a chunk admitted, then the importer dies
/// before its checkpoint and admits the same chunk again on resume.
#[tokio::test]
async fn an_import_chunk_retried_after_a_crash_is_stored_once() {
    let dir = TempDir::new().unwrap();
    let chunk = vec![imported_event(1), imported_event(2)];
    // The importer died after the chunk committed and before its step checkpoint. Closing
    // instead of dropping keeps a leftover transfer task from taking the reopened inbox's batch.
    let storage = Storage::open(dir.path(), test_budgets()).await.unwrap();
    assert!(storage.admit_import("events:1", &chunk).await.unwrap());
    storage.close().await.unwrap();
    let storage = Storage::open(dir.path(), test_budgets()).await.unwrap();
    assert!(!storage.admit_import("events:1", &chunk).await.unwrap());
    let changed = storage.admit_import("events:1", &chunk[..1]).await;
    assert!(
        changed.is_err(),
        "a changed chunk under the same key is refused"
    );
    assert!(
        storage
            .admit_import("events:2", &[imported_event(3)])
            .await
            .unwrap()
    );
    storage.drain().await;
    let counts = storage
        .read(Lane::Report, |s| {
            s.connection()
                .query_row("SELECT count(*), count(DISTINCT id) FROM events", [], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
                })
        })
        .await
        .unwrap()
        .value;
    assert_eq!(counts, (3, 3));
    storage.close().await.unwrap();
}
