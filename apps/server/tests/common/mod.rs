//! Test harness: a real router over a temporary data directory, seeded from `fixtures/access`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use holocron_protocol::access::{InstallRole, Role, WorkspaceKind};
use holocron_protocol::ids::{SiteId, UserId, WorkspaceId};
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, Config};
use holocron_server::db::{Db, new_id, now_ms};
use holocron_server::identity::{self, SESSION_COOKIE};
use holocron_server::{AppState, router, sites};
use holocron_storage::{Budgets, Lane, Storage, duckdb};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

pub mod wire;

pub const HOST: &str = "holocron.test";
pub const ORIGIN: &str = "http://holocron.test";
pub const PASSWORD: &str = "correct horse battery";

/// Small engine budgets, no disk reserve, and a short transfer delay.
pub fn test_budgets() -> Budgets {
    Budgets {
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        transfer_delay: std::time::Duration::from_millis(5),
        ..Budgets::default()
    }
}

pub struct Harness {
    pub state: AppState,
    router: Router,
    /// Alias from the access fixture to the generated ID.
    pub users: HashMap<String, UserId>,
    pub sites: HashMap<String, SiteId>,
    pub workspaces: HashMap<String, WorkspaceId>,
    data: TempDir,
    pub web: TempDir,
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl Answer {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    pub fn code(&self) -> String {
        self.json()["error_code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    pub fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    }
}

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/access")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

impl Harness {
    pub async fn new() -> Self {
        Self::with(CollectConfig::default(), test_budgets(), Clock::system()).await
    }

    /// A harness with its own collection settings, storage budgets and clock.
    pub async fn with(collect: CollectConfig, budgets: Budgets, clock: Clock) -> Self {
        // A GeoIP download in a test fails at once instead of fetching DB-IP's file.
        let collect = CollectConfig {
            geoip_download: "http://127.0.0.1:9".into(),
            ..collect
        };
        let data = TempDir::new().unwrap();
        let web = TempDir::new().unwrap();
        std::fs::create_dir_all(web.path().join("assets")).unwrap();
        std::fs::write(
            web.path().join("index.html"),
            "<!doctype html><div id=app></div>",
        )
        .unwrap();
        std::fs::write(web.path().join("assets/app-3f9a1c.js"), "console.log(1)").unwrap();

        let db = Db::open(data.path()).await.unwrap();
        let config = Config {
            data_dir: data.path().to_path_buf(),
            bind: "127.0.0.1:0".parse().unwrap(),
            web_dir: Some(PathBuf::from(web.path())),
            public_origin: None,
            instance_name: "Test install".into(),
            setup_token: None,
            collect,
            disk_reserve_bytes: budgets.disk_reserve_bytes,
        };
        let storage = Arc::new(Storage::open(data.path(), budgets).await.unwrap());
        let state = AppState::new(db, storage, config, clock).await.unwrap();
        let mut harness = Self {
            router: router(state.clone()),
            state,
            users: HashMap::new(),
            sites: HashMap::new(),
            workspaces: HashMap::new(),
            data,
            web,
        };
        harness.seed_access_fixture().await;
        harness
    }

    /// Users, workspaces, memberships and sites of `fixtures/access`. Every user has [`PASSWORD`].
    async fn seed_access_fixture(&mut self) {
        let resources = fixture("resources.json");
        let principals = fixture("principals.json");
        let hash = identity::hash_password(PASSWORD);
        let mut tx = self.state.db.writer.begin().await.unwrap();

        for workspace in resources["workspaces"].as_array().unwrap() {
            let alias = workspace["id"].as_str().unwrap();
            let kind = WorkspaceKind::parse(workspace["kind"].as_str().unwrap()).unwrap();
            let id = sites::create_workspace(&mut tx, workspace["name"].as_str().unwrap(), kind)
                .await
                .unwrap();
            self.workspaces.insert(alias.into(), id);
        }
        for (index, site) in resources["sites"].as_array().unwrap().iter().enumerate() {
            let alias = site["id"].as_str().unwrap();
            let workspace = &self.workspaces[site["workspace"].as_str().unwrap()];
            let id = sites::create_site(
                &mut tx,
                workspace,
                &format!("Site {index}"),
                "example.test",
                "Europe/Vienna",
            )
            .await
            .unwrap();
            self.sites.insert(alias.into(), id);
        }
        self.sites
            .insert("site_guess".into(), SiteId::new(new_id()));

        for principal in principals["principals"].as_array().unwrap() {
            if principal["kind"] != "user" {
                continue;
            }
            let alias = principal["id"].as_str().unwrap();
            let role = InstallRole::parse(principal["install_role"].as_str().unwrap()).unwrap();
            let id = new_id();
            sqlx::query(
                "INSERT INTO users (id, email, name, password_hash, install_role, created_at) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(format!("{alias}@example.test"))
            .bind(alias)
            .bind(&hash)
            .bind(role.as_str())
            .bind(now_ms())
            .execute(&mut *tx)
            .await
            .unwrap();
            let user = UserId::new(id);
            for (workspace, role) in principal["memberships"].as_object().unwrap() {
                let role = Role::parse(role.as_str().unwrap()).unwrap();
                sites::add_member(&mut tx, &self.workspaces[workspace], &user, role)
                    .await
                    .unwrap();
            }
            self.users.insert(alias.into(), user);
        }
        tx.commit().await.unwrap();
    }

    /// A session cookie for a fixture user, created directly so tests skip password hashing.
    /// Reauthenticated just now, as the access fixture's `default_session` says.
    pub async fn session(&self, alias: &str) -> String {
        self.session_reauthenticated(alias, Some(now_ms())).await
    }

    /// A session cookie whose last password check was at `reauthenticated_at`, or never.
    pub async fn session_reauthenticated(
        &self,
        alias: &str,
        reauthenticated_at: Option<i64>,
    ) -> String {
        let (token, digest) = identity::new_session_token();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, created_at, expires_at, last_seen_at, reauthenticated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(digest)
        .bind(self.users[alias].as_str())
        .bind(now_ms())
        .bind(now_ms() + 60_000)
        .bind(now_ms())
        .bind(reauthenticated_at)
        .execute(&self.state.db.writer)
        .await
        .unwrap();
        format!("{SESSION_COOKIE}={token}")
    }

    /// An API key of a fixture user, inserted directly. `sites: None` reaches every site the
    /// owner can read. Returns the `Authorization` header value.
    pub async fn api_key(
        &self,
        owner: &str,
        operations: &[&str],
        sites: Option<&[&str]>,
    ) -> String {
        let (secret, digest) = identity::new_session_token();
        let id = new_id();
        let mut tx = self.state.db.writer.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO api_keys (id, owner_id, name, prefix, secret_hash, all_sites, created_at)
             VALUES (?, ?, 'test', ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(self.users[owner].as_str())
        .bind(&secret[..8])
        .bind(digest)
        .bind(sites.is_none())
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .unwrap();
        for operation in operations {
            sqlx::query("INSERT INTO api_key_operations (key_id, operation) VALUES (?, ?)")
                .bind(&id)
                .bind(operation)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        for site in sites.unwrap_or_default() {
            sqlx::query("INSERT INTO api_key_sites (key_id, site_id) VALUES (?, ?)")
                .bind(&id)
                .bind(self.sites[*site].as_str())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        format!("Bearer {secret}")
    }

    /// A share inserted directly. `target` is the JSON of a protocol `ShareTarget` with real
    /// IDs. Returns the `Authorization` header value.
    pub async fn share(
        &self,
        workspace: &str,
        target: Value,
        export: bool,
        recordings: bool,
        expires_at: i64,
    ) -> String {
        let (token, digest) = identity::new_session_token();
        let (site, board) = match target["kind"].as_str().unwrap() {
            "exploration" => (target["state"]["site"].as_str().map(str::to_owned), None),
            _ => (None, target["board"].as_str().map(str::to_owned)),
        };
        sqlx::query(
            "INSERT INTO shares (id, workspace_id, site_id, board_id, name, target, export, recordings,
                                 prefix, token_hash, created_by, created_at, updated_at, expires_at)
             VALUES (?, ?, ?, ?, 'test', ?, ?, ?, ?, ?, 'test', ?, ?, ?)",
        )
        .bind(new_id())
        .bind(self.workspaces[workspace].as_str())
        .bind(site)
        .bind(board)
        .bind(target.to_string())
        .bind(export)
        .bind(recordings)
        .bind(&token[..8])
        .bind(digest)
        .bind(now_ms())
        .bind(now_ms())
        .bind(expires_at)
        .execute(&self.state.db.writer)
        .await
        .unwrap();
        format!("Share {token}")
    }

    /// Removes a fixture user from a workspace, as AX-S004 and AX-S011 do mid-session.
    pub async fn remove_member(&self, alias: &str, workspace: &str) {
        sqlx::query("DELETE FROM memberships WHERE user_id = ? AND workspace_id = ?")
            .bind(self.users[alias].as_str())
            .bind(self.workspaces[workspace].as_str())
            .execute(&self.state.db.writer)
            .await
            .unwrap();
    }

    pub async fn send(&self, request: Request<Body>) -> Answer {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
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

    pub async fn get(&self, path: &str) -> Answer {
        self.send(
            Request::get(path)
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn query(&self, cookie: Option<&str>, name: &str, query: &str) -> Answer {
        let mut request =
            Request::get(format!("/api/queries/{name}?{query}")).header(header::HOST, HOST);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A query with an `Authorization` header (API key or share), no cookie.
    pub async fn query_with(&self, authorization: &str, name: &str, query: &str) -> Answer {
        let request = Request::get(format!("/api/queries/{name}?{query}"))
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, authorization);
        self.send(request.body(Body::empty()).unwrap()).await
    }

    /// A command as a script sends it with an API key: no Origin, no request marker.
    pub async fn command_with(&self, authorization: &str, name: &str, body: Value) -> Answer {
        let request = Request::post(format!("/api/commands/{name}"))
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, authorization)
            .header(header::CONTENT_TYPE, "application/json");
        self.send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    /// A command as the web client sends it: same origin, request marker, JSON body.
    pub async fn command(&self, cookie: Option<&str>, name: &str, body: Value) -> Answer {
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

    pub fn data_dir(&self) -> &Path {
        self.data.path()
    }

    /// A collection request from `peer` with a desktop browser user agent.
    pub async fn collect(
        &self,
        method: &str,
        path: &str,
        peer: &str,
        body: Option<Value>,
    ) -> Answer {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(
                header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
            )
            .header(header::CONTENT_TYPE, "application/json")
            .extension(axum::extract::ConnectInfo(
                peer.parse::<std::net::SocketAddr>().unwrap(),
            ));
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        self.send(request.body(body).unwrap()).await
    }

    /// Creates a site in workspace `ws_a`, gives analytics its session policy, and
    /// returns its ID.
    pub async fn site(&self, timezone: &str) -> String {
        let workspace = self.workspaces["ws_a"].clone();
        let mut conn = self.state.db.writer.acquire().await.unwrap();
        let site = sites::create_site(&mut conn, &workspace, "Collected", "example.test", timezone)
            .await
            .unwrap();
        sites::policy::mirror(&mut conn, self.state.collector.storage(), site.as_str())
            .await
            .unwrap();
        site.as_str().to_owned()
    }

    /// Runs `f` on an analytical snapshot once every admitted record is applied.
    pub async fn analytics<T: Send + 'static>(
        &self,
        f: impl FnOnce(&duckdb::Connection) -> duckdb::Result<T> + Send + 'static,
    ) -> T {
        let storage = self.state.collector.storage();
        storage.drain().await;
        storage
            .read(Lane::Live, move |s| f(s.connection()))
            .await
            .unwrap()
            .value
    }

    pub async fn topic_head(&self, topic: &str) -> i64 {
        sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM topic_events WHERE topic = ?")
            .bind(topic)
            .fetch_one(&self.state.db.reader)
            .await
            .unwrap()
    }
}

impl Harness {
    /// Moves the last password check of the session behind `cookie` (`name=token`), as if the
    /// clock had advanced since it (AX-S025).
    pub async fn set_reauthenticated_at(&self, cookie: &str, at: Option<i64>) {
        let token = cookie.split_once('=').unwrap().1;
        sqlx::query("UPDATE sessions SET reauthenticated_at = ? WHERE id = ?")
            .bind(at)
            .bind(identity::token_digest(token))
            .execute(&self.state.db.writer)
            .await
            .unwrap();
    }
}

/// The pinned state of `exp_a1` in `fixtures/access`: page views and visitor days by path and
/// country of mobile traffic over the last `days` days.
pub fn share_state(site: &str, days: u16) -> Value {
    serde_json::json!({
        "v": 1, "site": site, "measures": ["pageviews", "visitor_days"], "dimensions": ["path", "country"],
        "filter": { "kind": "group", "op": "and", "items": [
            { "kind": "cond", "id": "c1", "field": "device", "cmp": "eq", "value": { "t": "string", "v": "mobile" } }
        ] },
        "range": { "kind": "relative", "unit": "day", "count": days },
        "timezone": "UTC", "granularity": "day", "compare": "none", "metric_version": 1,
        "sort": [], "page": { "size": 50, "index": 0 }, "visualization": "table"
    })
}

/// The `share_a1` target over `state`: its measures and dimensions, `range_max_days`, the
/// state's filter pinned, narrowing allowed.
pub fn exploration_share(state: &Value, range_max_days: u16) -> Value {
    serde_json::json!({
        "kind": "exploration",
        "state": state,
        "limits": {
            "measures": state["measures"], "dimensions": state["dimensions"], "properties": [],
            "range_max_days": range_max_days, "pinned_filter": state["filter"], "narrowing_allowed": true
        }
    })
}

impl Harness {
    /// Opens `GET /api/subscribe` on the site's analytics topic with a credential header:
    /// `("cookie", ...)` or `("authorization", ...)`. The body streams server-sent events.
    pub async fn subscribe(
        &self,
        credential: (&str, &str),
        site: &str,
    ) -> axum::response::Response {
        let request = Request::get(format!("/api/subscribe?topic=analytics%3A{site}"))
            .header(header::HOST, HOST)
            .header(credential.0, credential.1)
            .body(Body::empty())
            .unwrap();
        self.router.clone().oneshot(request).await.unwrap()
    }
}

/// The next `count` server-sent event frames of a stream, each within 10 seconds.
pub async fn frames(mut body: Body, count: usize) -> Vec<Value> {
    let mut text = String::new();
    let mut out = Vec::new();
    while out.len() < count {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(10), body.frame())
            .await
            .expect("a frame in time")
            .expect("the stream stays open")
            .unwrap();
        if let Ok(data) = frame.into_data() {
            text.push_str(&String::from_utf8_lossy(&data));
        }
        while let Some(end) = text.find("\n\n") {
            let event: String = text.drain(..end + 2).collect();
            if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data: ")) {
                out.push(serde_json::from_str(data).unwrap());
            }
        }
    }
    out
}
