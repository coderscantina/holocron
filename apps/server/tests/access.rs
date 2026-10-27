//! The access fixture over HTTP (milestone 10): every row of `fixtures/access/cases.json` for
//! every principal, the check that each protected operation of the registry has rows for every
//! principal class, the `scenarios.json` items no other suite runs, and site access, sessions
//! and bootstrap.

mod common;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use common::{Answer, HOST, Harness, ORIGIN, PASSWORD, exploration_share, frames, share_state};
use holocron_protocol::access::Role;
use holocron_protocol::ids::UserId;
use holocron_protocol::operations;
use holocron_protocol::registry::{Access, Kind, OperationInfo};
use holocron_server::db::{new_id, now_ms};
use holocron_server::identity::totp;
use holocron_server::init::{self, InitArgs};
use holocron_server::sites::scope;
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;
const NEW_PASSWORD: &str = "a much longer new password";
const DESKTOP_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
/// Fixture operations that the registry does not declare yet. Their rows are reported as
/// pending, never edited.
const PENDING: &[&str] = &["sessions.list"];
/// Operations whose effect is deferred to a later milestone: allowed rows must pass every
/// access check and then meet the handler's `internal` "not implemented" refusal.
const DEFERRED: &[(&str, &str)] = &[];

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/access")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn find(name: &str) -> Option<&'static OperationInfo> {
    operations::find(Kind::Command, name).or_else(|| operations::find(Kind::Query, name))
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn ok(answer: Answer) -> Value {
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

fn base32_decode(input: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buffer, mut bits, mut out) = (0u32, 0, Vec::new());
    for c in input.bytes() {
        let value = ALPHABET.iter().position(|a| *a == c).unwrap() as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

/// How a principal presents itself.
#[derive(Clone)]
enum Credential {
    Anonymous,
    Cookie(String),
    /// An `Authorization` header: an API key or a share token.
    Header(String),
}

/// One fresh seed of `fixtures/access`: the harness's users, workspaces and sites, plus the
/// explorations, boards, shares, keys, definitions, invitations, 2FA states and recordings the
/// rows name. Fixture revisions of `board_a1` are relative to its seeded revision 7.
struct World {
    h: Harness,
    /// Resource alias to generated ID, beyond users, sites and workspaces.
    ids: HashMap<&'static str, String>,
    /// Invitation codes by alias.
    codes: HashMap<&'static str, String>,
    /// `Authorization` header values of the key and share principals.
    credentials: HashMap<&'static str, String>,
    /// `u_member` and the other user aliases of `principals.json` to principal aliases.
    user_aliases: HashMap<String, String>,
    /// The cards of `board_a1`, as `boards.update` takes them.
    cards: Value,
    /// `board_a1`'s revision after seeding, which the fixture calls 7.
    board_revision: i64,
}

impl World {
    /// With `traffic`, site_a1 also gets recordings and an event carrying the property `plan`.
    async fn new(traffic: bool) -> Self {
        let h = Harness::new().await;
        let user_aliases = fixture("principals.json")["principals"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| Some((p["user"].as_str()?.to_owned(), p["id"].as_str()?.to_owned())))
            .chain([("u_leaver".to_owned(), "u_leaver".to_owned())])
            .collect();
        let mut world = Self {
            h,
            ids: HashMap::new(),
            codes: HashMap::new(),
            credentials: HashMap::new(),
            user_aliases,
            cards: Value::Null,
            board_revision: 0,
        };
        if traffic {
            world.seed_traffic().await;
        }
        world.seed().await;
        world.board_revision = world.board_head().await;
        world
    }

    fn site(&self, alias: &str) -> String {
        self.h.sites[alias].to_string()
    }

    fn workspace(&self, alias: &str) -> String {
        self.h
            .workspaces
            .get(alias)
            .map_or_else(new_id, ToString::to_string)
    }

    /// The generated ID of a resource alias; a `*_missing` alias is a well-formed unused ID.
    fn id(&self, alias: &str) -> String {
        self.ids.get(alias).cloned().unwrap_or_else(new_id)
    }

    /// The user ID behind `u_member`, `p_member` and the like.
    fn user(&self, alias: &str) -> String {
        let principal = self.user_aliases.get(alias).map_or(alias, String::as_str);
        self.h.users[principal].to_string()
    }

    /// A command as the web client sends it, with a fresh idempotency key, that must succeed.
    async fn run(&self, cookie: &str, op: &str, args: Value) -> Value {
        ok(self
            .send(
                Kind::Command,
                op,
                &Credential::Cookie(cookie.into()),
                args,
                None,
            )
            .await)
    }

    async fn send(
        &self,
        kind: Kind,
        op: &str,
        credential: &Credential,
        args: Value,
        idempotency_key: Option<&str>,
    ) -> Answer {
        let mut request = match kind {
            Kind::Command => Request::post(format!("/api/commands/{op}"))
                .header(header::ORIGIN, ORIGIN)
                .header("x-holocron-request", "1")
                .header(header::CONTENT_TYPE, "application/json"),
            // A report query is too big for a query string; it is sent as a JSON body.
            Kind::Query if op == "analytics.report" => Request::post(format!("/api/queries/{op}"))
                .header(header::CONTENT_TYPE, "application/json"),
            Kind::Query => Request::get(format!("/api/queries/{op}?{}", form(&args))),
        }
        .header(header::HOST, HOST);
        match credential {
            Credential::Anonymous => {}
            Credential::Cookie(cookie) => request = request.header(header::COOKIE, cookie),
            Credential::Header(value) => request = request.header(header::AUTHORIZATION, value),
        }
        let body = match kind {
            Kind::Command => {
                let mut body = args;
                body["idempotency_key"] = json!(
                    idempotency_key.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned)
                );
                Body::from(body.to_string())
            }
            Kind::Query if op == "analytics.report" => Body::from(args.to_string()),
            Kind::Query => Body::empty(),
        };
        self.h.send(request.body(body).unwrap()).await
    }

    async fn seed(&mut self) {
        // `u_leaver`: a viewer of ws_a who owns nothing, the target of `users.*` rows.
        let leaver = new_id();
        let mut tx = self.h.state.db.writer.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, install_role, created_at)
             SELECT ?, 'u_leaver@example.test', 'u_leaver', password_hash, 'user', ? FROM users LIMIT 1",
        )
        .bind(&leaver)
        .bind(now_ms())
        .execute(&mut *tx)
        .await
        .unwrap();
        holocron_server::sites::add_member(
            &mut tx,
            &self.h.workspaces["ws_a"],
            &UserId::new(leaver.clone()),
            Role::Viewer,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        self.h.users.insert("u_leaver".into(), UserId::new(leaver));

        let member = self.h.session("p_member").await;
        let admin = self.h.session("p_admin").await;
        let owner = self.h.session("p_owner").await;
        let site = self.site("site_a1");
        let state = share_state(&site, 30);

        let saved = self
            .run(
                &member,
                "explorations.save",
                json!({ "site": site, "name": "Mobile pages", "state": state }),
            )
            .await;
        self.ids.insert(
            "exp_a1",
            saved["exploration"]["id"].as_str().unwrap().into(),
        );

        let board = self
            .run(
                &member,
                "boards.create",
                json!({ "workspace": self.workspace("ws_a"), "name": "Overview" }),
            )
            .await["board"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut pinned = Value::Null;
        for card_site in ["site_a1", "site_a2"] {
            let card = json!({ "board": board, "title": card_site, "state": share_state(&self.site(card_site), 30) });
            pinned = self.run(&member, "boards.pin", card).await;
        }
        self.cards = json!(
            pinned["board"]["cards"]
                .as_array()
                .unwrap()
                .iter()
                .map(|card| json!({ "id": card["id"], "title": card["title"] }))
                .collect::<Vec<_>>()
        );
        self.ids.insert("board_a1", board.clone());
        let other = self.h.session("p_other_owner").await;
        let board_b1 = self
            .run(
                &other,
                "boards.create",
                json!({ "workspace": self.workspace("ws_b"), "name": "Beta" }),
            )
            .await["board"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let card = json!({ "board": board_b1, "title": "b1", "state": share_state(&self.site("site_b1"), 30) });
        self.run(&other, "boards.pin", card).await;
        self.ids.insert("board_b1", board_b1);

        let expires_at = now_ms() + 30 * DAY_MS;
        let exploration = |recordings: bool| {
            json!({ "name": "Mobile pages", "target": exploration_share(&state, 30), "export": false,
                    "recordings": recordings, "expires_at": expires_at })
        };
        for (alias, principal, args) in [
            ("share_a1", "p_share_viewer", exploration(false)),
            ("share_a1_rec", "p_share_viewer_rec", exploration(true)),
            ("share_revoked", "p_share_revoked", exploration(false)),
            (
                "share_board_a1",
                "p_share_board",
                json!({ "name": "Board", "target": { "kind": "board", "board": board, "sites": [site] },
                        "export": false, "recordings": false, "expires_at": expires_at }),
            ),
        ] {
            let created = self.run(&member, "sharing.create", args).await;
            let id = created["share"]["id"].as_str().unwrap().to_owned();
            self.credentials.insert(
                principal,
                format!("Share {}", created["secret"].as_str().unwrap()),
            );
            self.ids.insert(alias, id);
        }
        self.run(
            &member,
            "sharing.revoke",
            json!({ "share": self.id("share_revoked") }),
        )
        .await;
        let expired = self
            .h
            .share(
                "ws_a",
                exploration_share(&state, 30),
                false,
                false,
                now_ms() - 1,
            )
            .await;
        self.credentials.insert("p_share_expired", expired);

        let key_site = self
            .h
            .api_key(
                "p_member",
                &[
                    "analytics.explore",
                    "analytics.export",
                    "fields.values",
                    "realtime.snapshot",
                ],
                Some(&["site_a1"]),
            )
            .await;
        let queries: Vec<&str> = operations::all()
            .filter(|op| op.kind == Kind::Query && op.policy.key_grantable)
            .map(|op| op.name)
            .collect();
        let key_readonly = self.h.api_key("p_admin", &queries, None).await;
        let key_revoked = self
            .h
            .api_key("p_admin", &["analytics.explore"], Some(&["site_a1"]))
            .await;
        sqlx::query("UPDATE api_keys SET revoked_at = ? WHERE prefix = ?")
            .bind(now_ms())
            .bind(key_prefix(&key_revoked))
            .execute(&self.h.state.db.writer)
            .await
            .unwrap();
        let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys WHERE prefix = ?")
            .bind(key_prefix(&key_site))
            .fetch_one(&self.h.state.db.reader)
            .await
            .unwrap();
        self.ids.insert("key_site", key_id);
        self.credentials.insert("p_key_site", key_site);
        self.credentials.insert("p_key_readonly", key_readonly);
        self.credentials.insert("p_key_revoked", key_revoked);

        let goal = json!({ "site": site, "name": "Signup", "definition": {
            "predicate": { "kind": "group", "op": "and", "items": [
                { "kind": "cond", "id": "g1", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "signup" } }] },
            "scope": "session", "counting": "once" } });
        let goal = self.run(&member, "goals.save", goal).await;
        self.ids
            .insert("goal_a1", goal["goal"]["id"].as_str().unwrap().into());
        let workspace = self.workspace("ws_a");
        let link = self
            .run(
                &member,
                "links.create",
                json!({ "workspace": workspace, "name": "Spring", "url": "https://example.test/", "slug": "spring" }),
            )
            .await;
        self.ids
            .insert("link_a1", link["link"]["id"].as_str().unwrap().into());
        let pixel = self
            .run(
                &member,
                "pixels.create",
                json!({ "workspace": workspace, "name": "Newsletter", "slug": "newsletter" }),
            )
            .await;
        self.ids
            .insert("pixel_a1", pixel["pixel"]["id"].as_str().unwrap().into());
        let segment = self
            .run(
                &member,
                "segments.save",
                json!({ "site": site, "name": "Mobile", "filter": state["filter"] }),
            )
            .await;
        self.ids.insert(
            "segment_a1",
            segment["segment"]["id"].as_str().unwrap().into(),
        );
        let annotation = self
            .run(
                &member,
                "annotations.create",
                json!({ "site": site, "at": now_ms() - DAY_MS, "text": "Launch" }),
            )
            .await;
        self.ids.insert(
            "annotation_a1",
            annotation["annotation"]["id"].as_str().unwrap().into(),
        );
        let recalculation = self
            .run(
                &owner,
                "recalculations.start",
                json!({ "site": site, "first_day": "2026-09-30", "last_day": "2026-10-01" }),
            )
            .await;
        let collection_key = self
            .run(
                &member,
                "collectionKeys.create",
                json!({ "site": site, "name": "Backend" }),
            )
            .await;
        self.ids.insert(
            "collection_key_a1",
            collection_key["key"]["id"].as_str().unwrap().into(),
        );
        self.ids.insert(
            "recalc_a1",
            recalculation["recalculation"]["id"]
                .as_str()
                .unwrap()
                .into(),
        );

        for alias in ["invitation_a1", "invitation_revoked", "invitation_expired"] {
            let created = self
                .run(
                    &admin,
                    "teams.invite",
                    json!({ "workspace": self.workspace("ws_a"), "role": "member", "label": alias }),
                )
                .await;
            let id = created["invitation"]["id"].as_str().unwrap().to_owned();
            self.codes
                .insert(alias, created["secret"].as_str().unwrap().into());
            self.ids.insert(alias, id);
        }
        self.run(
            &admin,
            "teams.revokeInvitation",
            json!({ "invitation": self.id("invitation_revoked") }),
        )
        .await;
        sqlx::query("UPDATE invitations SET expires_at = ? WHERE id = ?")
            .bind(now_ms() - 1)
            .bind(self.id("invitation_expired"))
            .execute(&self.h.state.db.writer)
            .await
            .unwrap();

        let enrolled = self.h.session("p_2fa_enrolled").await;
        self.enroll(&enrolled).await;
        let install_admin = self.h.session("p_install_admin").await;
        self.run(
            &install_admin,
            "security.updatePolicy",
            json!({ "target": { "scope": "user", "user": self.user("p_2fa_unenrolled") }, "two_factor_required": true }),
        )
        .await;
    }

    /// Begins and confirms a TOTP factor for the person behind `cookie`. Returns the secret.
    async fn enroll(&self, cookie: &str) -> Vec<u8> {
        let begin = self.run(cookie, "twoFactor.begin", json!({})).await;
        let secret = base32_decode(begin["secret"]["key"].as_str().unwrap());
        let code = totp::code(&secret, totp::step(now_ms()));
        self.run(cookie, "twoFactor.confirm", json!({ "code": code }))
            .await;
        secret
    }

    /// Replay on for site_a1, then `rec_a1` from a phone, inside the pinned filter of
    /// `share_a1_rec`, and `rec_a1_old` from a desktop, outside it, and an event with the
    /// property `plan`. Replays older than the 30-day share range are past replay retention for
    /// everyone, so the filter is what puts `rec_a1_old` outside the share.
    async fn seed_traffic(&mut self) {
        let site = self.site("site_a1");
        let owner = self.h.session("p_owner").await;
        self.run(
            &owner,
            "recordings.updatePolicy",
            json!({ "site": site, "replay_enabled": true, "heatmap_enabled": true, "replay_sample_percent": 100,
                    "heatmap_sample_percent": 100, "max_duration_minutes": 5, "excluded_elements": [], "excluded_paths": [] }),
        )
        .await;
        let now = now_ms();
        for (agent, peer) in [
            (DESKTOP_UA, "198.51.100.40:1"),
            (MOBILE_UA, "198.51.100.41:1"),
        ] {
            let hit = json!({ "type": "event", "payload": { "website": site, "url": "/", "hostname": "shop.test" } });
            let cache = ok(self.tracker(agent, peer, None, hit).await)["cache"]
                .as_str()
                .unwrap()
                .to_owned();
            let event = json!({ "type": 4, "timestamp": now, "data": { "href": "https://shop.test/", "width": 390, "height": 844 } });
            let record = json!({ "type": "record", "payload": { "website": site, "events": [event], "timestamp": 1 } });
            ok(self.tracker(agent, peer, Some(&cache), record).await);
        }
        let plan = json!({ "type": "event", "payload": { "website": site, "url": "/pricing", "hostname": "shop.test",
                                                        "name": "signup", "data": { "plan": "pro" } } });
        ok(self.tracker(MOBILE_UA, "198.51.100.42:1", None, plan).await);
        self.h.state.collector.storage().drain().await;
        holocron_analytics::run_until_clean(
            self.h.state.collector.storage(),
            holocron_analytics::Limits::default(),
        )
        .await
        .unwrap();
        let list = self
            .send(
                Kind::Query,
                "recordings.list",
                &Credential::Cookie(owner.clone()),
                json!({ "site": site, "start": now - 60 * DAY_MS, "end": now + DAY_MS }),
                None,
            )
            .await;
        for capture in ok(list)["captures"].as_array().unwrap() {
            let alias = match capture["device"].as_str() {
                Some("mobile") => "rec_a1",
                _ => "rec_a1_old",
            };
            self.ids
                .insert(alias, capture["id"].as_str().unwrap().into());
        }
        assert!(
            self.ids.contains_key("rec_a1") && self.ids.contains_key("rec_a1_old"),
            "{:?}",
            self.ids
        );
        // `sessions.get` rows read one of these sessions as `session_a1`.
        let timeline = self
            .send(
                Kind::Query,
                "sessions.timeline",
                &Credential::Cookie(owner),
                json!({ "site": site, "hours": 6 }),
                None,
            )
            .await;
        let session = ok(timeline)["sessions"][0]["session"].as_i64().unwrap();
        self.ids.insert("session_a1", session.to_string());
    }

    /// A tracker hit (`/api/send`) or, with the hit's cache token, a recording part.
    async fn tracker(&self, agent: &str, peer: &str, cache: Option<&str>, body: Value) -> Answer {
        let path = if cache.is_some() {
            "/api/record"
        } else {
            "/api/send"
        };
        let mut request = Request::post(path)
            .header(header::HOST, HOST)
            .header(header::USER_AGENT, agent)
            .header(header::CONTENT_TYPE, "application/json")
            .extension(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        if let Some(cache) = cache {
            request = request.header("x-umami-cache", cache);
        }
        self.h
            .send(request.body(Body::from(body.to_string())).unwrap())
            .await
    }

    /// The current head of the `ws_a` topic, which is `board_a1`'s revision.
    async fn board_head(&self) -> i64 {
        let topic = format!("workspace:{}", self.workspace("ws_a"));
        self.h.topic_head(&topic).await
    }

    /// The real revision for a fixture revision of `board_a1`.
    fn revision(&self, fixture: i64) -> i64 {
        fixture - 7 + self.board_revision
    }

    /// The credential of a row's principal, in the row's session state.
    async fn credential(&self, principal: &str, row: &Value) -> Credential {
        if row["resource"] == "unknown_token" {
            return Credential::Header("Share unknown-token".into());
        }
        if principal == "p_signed_out" {
            return Credential::Anonymous;
        }
        if let Some(header) = self.credentials.get(principal) {
            return Credential::Header(header.clone());
        }
        let cookie = match row["session"].as_str() {
            None => self.h.session(principal).await,
            Some("not reauthenticated") => self.h.session_reauthenticated(principal, None).await,
            Some(state) => {
                let minutes: i64 = state
                    .strip_prefix("reauthenticated ")
                    .and_then(|rest| rest.split(' ').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| panic!("unknown session state {state}"));
                self.h
                    .session_reauthenticated(principal, Some(now_ms() - minutes * MINUTE_MS))
                    .await
            }
        };
        Credential::Cookie(cookie)
    }

    /// The pinned state of `exp_a1` on `site`, changed as a row's `args.state` describes.
    fn described_state(&self, description: Option<&str>, site: &str) -> Value {
        let mut state = share_state(site, 30);
        let country = json!({ "kind": "cond", "id": "c2", "field": "country", "cmp": "eq", "value": { "t": "string", "v": "AT" } });
        let plan = json!([{ "scope": "event", "key": "plan" }]);
        match description.unwrap_or("pinned") {
            "pinned" | "pinned state of exp_a1" => {}
            "pinned filter AND country eq AT" => {
                state["filter"]["items"]
                    .as_array_mut()
                    .unwrap()
                    .push(country);
            }
            "filter = OR(pinned, country eq AT)" => {
                state["filter"]["op"] = json!("or");
                state["filter"]["items"]
                    .as_array_mut()
                    .unwrap()
                    .push(country);
            }
            "filter removed" => state["filter"] = Value::Null,
            "pinned, relative 7 days" => state["range"]["count"] = json!(7),
            "pinned, relative 90 days" => state["range"]["count"] = json!(90),
            "pinned, days 2026-01-01 to 2026-03-01 (60 days)" => {
                state["range"] =
                    json!({ "kind": "days", "start": "2026-01-01", "end": "2026-03-01" });
            }
            "pinned, relative 30 days, compare previous_period" => {
                state["compare"] = json!("previous_period");
            }
            "pinned, relative 15 days, compare previous_period" => {
                state["range"]["count"] = json!(15);
                state["compare"] = json!("previous_period");
            }
            "pinned, relative 16 days, compare previous_period" => {
                state["range"]["count"] = json!(16);
                state["compare"] = json!("previous_period");
            }
            "pinned, measures [revenue_net]" => state["measures"] = json!(["revenue_net"]),
            "pinned, dimensions [city]" => state["dimensions"] = json!(["city"]),
            "pinned, dimensions [{scope:event,key:plan}]" | "dimension is event property plan" => {
                state["dimensions"] = plan;
            }
            "pinned but state.site = site_a2" => state["site"] = json!(self.site("site_a2")),
            "pinned but state.site = site_b1" => state["site"] = json!(self.site("site_b1")),
            other => panic!("unknown state description {other}"),
        }
        state
    }

    /// Wire arguments for a row, mapped from the fixture's aliases and short forms as decision
    /// 0012 (Consequences) describes. `principal` is the caller.
    fn args(&self, row: &Value, principal: &str) -> Value {
        let op = row["op"].as_str().unwrap();
        let resource = row["resource"].as_str().unwrap();
        let given = &row["args"];
        let now = now_ms();
        let site = match given["site"].as_str().or(Some(resource)) {
            Some(alias) if alias.starts_with("site_") => self.site(alias),
            _ => self.site("site_a1"),
        };
        let state = || self.described_state(given["state"].as_str(), &site);
        let user = |alias: &str| self.user(alias);
        let window = json!({ "site": site, "start": now - 60 * DAY_MS, "end": now + DAY_MS });
        let with = |mut base: Value, extra: Value| {
            base.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            base
        };
        let target_user = if resource.starts_with("u_") {
            user(resource)
        } else {
            user("u_leaver")
        };
        let own_email = if self.h.users.contains_key(principal) {
            format!("{principal}@example.test")
        } else {
            "nobody@example.test".into()
        };
        match op {
            "analytics.explore" => json!({ "state": state().to_string() }),
            "analytics.report" => json!({ "v": 1, "blocks": [{ "name": "views", "site": site,
                "range": { "kind": "relative", "unit": "day", "count": 7 },
                "body": { "kind": "cut", "measures": ["pageviews"] } }] }),
            // One repeated query key is a list of one.
            "summaries.forSites" => json!({ "sites": site }),
            "summaries.findings" => json!({ "site": site, "limit": 3 }),
            "sessions.timeline" => json!({ "site": site, "hours": 6 }),
            "sessions.get" => json!({ "site": site, "session": self.id("session_a1") }),
            "analytics.export" => json!({ "state": state().to_string(), "format": "csv" }),
            "fields.values" => {
                json!({ "site": site, "field": given.get("field").unwrap_or(&json!("path")).to_string() })
            }
            "realtime.snapshot"
            | "fields.catalog"
            | "explorations.list"
            | "segments.list"
            | "goals.list"
            | "sites.get"
            | "sites.identityPolicy"
            | "sites.trackingCode"
            | "collectionKeys.list"
            | "sites.delete"
            | "sites.reset"
            | "recordings.policy"
            | "recordings.budget"
            | "sharing.list" => {
                json!({ "site": site })
            }
            "recordings.list" | "heatmaps.pages" | "annotations.list" | "analytics.freshness" => {
                window
            }
            "heatmaps.cells" => with(window, json!({ "path": "/", "viewport": "desktop" })),
            "recordings.get" | "recordings.save" | "recordings.unsave" => {
                json!({ "site": site, "capture": self.id(resource) })
            }
            "explorations.get" | "explorations.delete" => {
                json!({ "site": site, "exploration": self.id(resource) })
            }
            "explorations.save" => json!({ "site": site, "name": "Saved", "state": state() }),
            "boards.get" | "boards.delete" => json!({ "board": self.id(resource) }),
            "boards.update" => {
                let cards = if resource == "board_a1" {
                    self.cards.clone()
                } else {
                    json!([])
                };
                let mut args = json!({ "board": self.id(resource), "name": given["layout"].as_str().unwrap_or("Renamed"), "cards": cards });
                if let Some(revision) = given["expected_revision"].as_i64() {
                    args["expected_revision"] = json!(self.revision(revision));
                }
                args
            }
            "boards.pin" => {
                let state = self.described_state(given["state"].as_str(), &self.site("site_a1"));
                json!({ "board": self.id(resource), "title": "Pinned", "state": state })
            }
            "boards.create" => {
                json!({ "workspace": self.workspace(resource), "name": "New board" })
            }
            "boards.list" | "teams.get" | "teams.invitations" | "teams.delete" => {
                json!({ "workspace": self.workspace(resource) })
            }
            "goals.save" => {
                let mut goal = json!({ "site": site, "name": "Checkout", "definition": {
                    "predicate": { "kind": "group", "op": "and", "items": [
                        { "kind": "cond", "id": "g1", "field": "event_name", "cmp": "eq", "value": { "t": "string", "v": "checkout" } }] },
                    "scope": "session", "counting": "once" } });
                if resource != "site_a1" && !resource.starts_with("site_") {
                    goal["goal"] = json!(self.id(resource));
                }
                goal
            }
            "goals.get" | "goals.archive" => json!({ "site": site, "goal": self.id(resource) }),
            "links.list" | "pixels.list" => json!({ "workspace": self.workspace(resource) }),
            "links.create" => {
                json!({ "workspace": self.workspace(resource), "name": "Summer", "url": "https://example.test/summer" })
            }
            "pixels.create" => json!({ "workspace": self.workspace(resource), "name": "Summer" }),
            "links.update" => {
                json!({ "link": self.id(resource), "name": "Renamed", "url": "https://example.test/", "slug": "renamed" })
            }
            "pixels.update" => {
                json!({ "pixel": self.id(resource), "name": "Renamed", "slug": "renamed" })
            }
            "links.delete" => json!({ "link": self.id(resource) }),
            "pixels.delete" => json!({ "pixel": self.id(resource) }),
            "links.explore" | "pixels.explore" => {
                let state = json!({
                    "v": 1, "site": self.id(resource), "measures": ["events", "visitors"], "dimensions": [],
                    "range": { "kind": "relative", "unit": "day", "count": 7 }, "timezone": "UTC",
                    "granularity": "day", "compare": "none", "metric_version": 1, "sort": [],
                    "page": { "size": 50, "index": 0 }, "visualization": "table"
                });
                json!({ "state": state.to_string() })
            }
            "segments.save" => {
                json!({ "site": site, "name": "Desktop", "filter": { "kind": "group", "op": "and", "items": [
                { "kind": "cond", "id": "d1", "field": "device", "cmp": "eq", "value": { "t": "string", "v": "desktop" } }] } })
            }
            "segments.delete" => json!({ "site": site, "segment": self.id(resource) }),
            "annotations.create" => json!({ "site": site, "at": now - DAY_MS, "text": "Note" }),
            "annotations.update" => {
                json!({ "site": site, "annotation": self.id(resource), "at": now - DAY_MS, "text": "Edited" })
            }
            "annotations.delete" => json!({ "site": site, "annotation": self.id(resource) }),
            "recalculations.start" => {
                json!({ "site": site, "first_day": "2026-09-30", "last_day": "2026-10-01" })
            }
            "recalculations.status" => {
                json!({ "site": site, "recalculation": self.id(resource) })
            }
            "recordings.updatePolicy" => {
                json!({ "site": site, "replay_enabled": true, "heatmap_enabled": false, "replay_sample_percent": 50,
                    "heatmap_sample_percent": 10, "max_duration_minutes": 5, "excluded_elements": [], "excluded_paths": [] })
            }
            "sites.update" => {
                json!({ "site": site, "name": "Renamed", "domain": "renamed.example" })
            }
            "sites.updateSessionPolicy" => {
                json!({ "site": site, "timezone": "Europe/Vienna", "window_minutes": 45 })
            }
            "sites.updateIdentityPolicy" => json!({ "site": site, "persistent_identity": true }),
            "collectionKeys.create" => json!({ "site": site, "name": "Backend" }),
            "collectionKeys.revoke" => json!({ "site": site, "key": self.id(resource) }),
            "sites.create" => {
                json!({ "workspace": self.workspace(resource), "name": "Shop", "domain": "shop.example", "timezone": "Europe/Vienna" })
            }
            "sites.transfer" => {
                let to = match given["direction"].as_str() {
                    Some("to_team") => {
                        json!({ "direction": "to_team", "team": self.workspace(given["team"].as_str().unwrap()) })
                    }
                    _ => {
                        let me = if self.h.users.contains_key(principal) {
                            principal
                        } else {
                            "p_owner"
                        };
                        json!({ "direction": "to_user", "user": user(me) })
                    }
                };
                json!({ "site": site, "to": to })
            }
            "sharing.create" => {
                json!({ "name": "Shared", "target": exploration_share(&share_state(&site, 30), 30), "export": false,
                    "recordings": false, "expires_at": now + 10 * DAY_MS })
            }
            "sharing.revoke" => json!({ "share": self.id(resource) }),
            "sharing.update" => {
                json!({ "share": self.id(resource), "name": "Renamed", "export": false, "recordings": false,
                    "expires_at": now + 10 * DAY_MS })
            }
            "teams.invite" => {
                json!({ "workspace": self.workspace(resource), "role": given["role"].as_str().unwrap_or("member"), "label": "row" })
            }
            "teams.join" => {
                json!({ "code": self.codes.get(resource).cloned().unwrap_or_else(|| "never-issued".into()) })
            }
            "teams.revokeInvitation" => json!({ "invitation": self.id(resource) }),
            "teams.update" => json!({ "workspace": self.workspace(resource), "name": "Renamed" }),
            "teams.create" => json!({ "name": "Side project" }),
            "members.updateRole" => {
                json!({ "workspace": self.workspace(resource), "user": user(given["user"].as_str().unwrap()), "role": given["role"] })
            }
            "members.remove" => {
                json!({ "workspace": self.workspace(resource), "user": user(given["user"].as_str().unwrap()) })
            }
            "apiKeys.create" => {
                let sites: Vec<String> = given["sites"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|alias| self.site(alias.as_str().unwrap()))
                    .collect();
                json!({ "name": "ci", "operations": given["operations"], "sites": { "kind": "only", "sites": sites } })
            }
            "apiKeys.revoke" => json!({ "key": self.id(resource) }),
            "account.update" => json!({ "name": "Renamed", "email": own_email }),
            "account.changePassword" => json!({ "new_password": NEW_PASSWORD }),
            "account.updatePreferences" => json!({ "timezone": "UTC" }),
            "auth.reauthenticate" => {
                let password = match given["password"].as_str() {
                    Some("wrong") => "wrong password",
                    _ => PASSWORD,
                };
                json!({ "password": password })
            }
            "security.updatePolicy" => {
                let target = match given["scope"].as_str().unwrap_or(resource) {
                    "installation" => json!({ "scope": "installation" }),
                    _ => json!({ "scope": "team", "workspace": self.workspace(resource) }),
                };
                json!({ "target": target, "two_factor_required": given.get("two_factor_required").unwrap_or(&json!(true)) })
            }
            "users.create" => {
                json!({ "email": "new.person@example.test", "name": "New", "install_role": "user" })
            }
            "users.update" => {
                let alias = self
                    .user_aliases
                    .get(resource)
                    .map_or(resource, String::as_str);
                json!({ "user": target_user, "name": "Renamed", "email": format!("{alias}@example.test"), "install_role": "user" })
            }
            "users.resetCredentials" => {
                json!({ "user": target_user, "password": true, "two_factor": false })
            }
            "users.delete" | "users.get" => json!({ "user": target_user }),
            "twoFactor.confirm" => json!({ "code": "000000" }),
            "sites.updateRetention" | "sites.retentionPreview" => {
                json!({ "site": site, "retention": given["retention"].as_str().unwrap_or("2y") })
            }
            "retention.updateDefault" | "retention.previewDefault" => {
                json!({ "retention": given["retention"].as_str().unwrap_or("2y") })
            }
            _ => json!({}),
        }
    }

    /// Runs one row and checks its expectation. Returns the answer for group comparisons.
    async fn check(&self, row: &Value) -> Result<Answer, String> {
        let id = row["id"].as_str().unwrap();
        let principal = row["principal"].as_str().unwrap();
        let op = find(row["op"].as_str().unwrap()).unwrap();
        match row["state"].as_str() {
            None => {}
            Some("after_removal") => self.h.remove_member(principal, "ws_a").await,
            Some("only_install_admin") => {
                sqlx::query("UPDATE users SET install_role = 'user' WHERE id = ?")
                    .bind(self.user("p_install_admin_member"))
                    .execute(&self.h.state.db.writer)
                    .await
                    .unwrap();
            }
            Some(other) => panic!("unknown principal state {other}"),
        }
        let credential = self.credential(principal, row).await;
        let mut args = self.args(row, principal);
        // The pending secret comes from a fresh session, so the row's session state applies to
        // the confirmation alone.
        if op.name == "twoFactor.confirm" && matches!(credential, Credential::Cookie(_)) {
            let fresh = self.h.session(principal).await;
            let begin = self.run(&fresh, "twoFactor.begin", json!({})).await;
            let secret = base32_decode(begin["secret"]["key"].as_str().unwrap());
            args["code"] = json!(totp::code(&secret, totp::step(now_ms())));
        }
        let answer = self.send(op.kind, op.name, &credential, args, None).await;

        let expect = &row["expect"];
        let got = (answer.status.as_u16(), answer.code());
        let deferred = DEFERRED.iter().any(|(name, _)| *name == op.name);
        let wanted = match expect["result"].as_str() {
            Some("allow") if deferred => (500, "internal".to_owned()),
            Some("allow") => (200, String::new()),
            _ => (
                expect["status"].as_u64().unwrap() as u16,
                expect["code"].as_str().unwrap().to_owned(),
            ),
        };
        if got != wanted {
            return Err(format!(
                "{id} {principal} {}: expected {wanted:?}, got {got:?}: {}",
                op.name,
                String::from_utf8_lossy(&answer.bytes)
            ));
        }
        if let Some(includes) = expect["body_includes"].as_str() {
            let alias = self.h.workspaces.get(includes).map(ToString::to_string);
            let needle = alias.as_deref().unwrap_or(includes);
            if !String::from_utf8_lossy(&answer.bytes).contains(needle) {
                return Err(format!("{id}: body lacks {includes}"));
            }
        }
        Ok(answer)
    }
}

/// The stored prefix of a key from its `Authorization` header value.
fn key_prefix(header: &str) -> &str {
    &header.strip_prefix("Bearer ").unwrap()[..8]
}

/// Query arguments as a query string. Strings go as they are, everything else as JSON text.
fn form(args: &Value) -> String {
    let empty = Map::new();
    args.as_object()
        .unwrap_or(&empty)
        .iter()
        .map(|(key, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            format!("{key}={}", encode(&text))
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// The principal class of `principals.json` kinds the coverage rule names.
fn class(kind: &str) -> &'static str {
    match kind {
        "user" => "user",
        "api_key" => "key",
        "share" => "share",
        "anonymous" => "signed-out",
        other => panic!("unexpected principal kind {other}"),
    }
}

fn principal_classes() -> HashMap<String, &'static str> {
    fixture("principals.json")["principals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["kind"] != "user_target_only")
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_owned(),
                class(p["kind"].as_str().unwrap()),
            )
        })
        .collect()
}

/// Milestone 10 acceptance: every row of `cases.json` runs through the HTTP dispatcher with
/// its principal's real credential. Queries share one seed and refused commands another, since
/// a refused command rolls back; both have `p_removed` removed already, as every `p_removed` row
/// runs after removal. Each allowed command gets a fresh seed.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn every_fixture_row_passes() {
    let cases = fixture("cases.json");
    let rows: Vec<Value> = cases["cases"].as_array().unwrap().clone();
    let classes = principal_classes();
    let (reads, refusals) = tokio::join!(World::new(true), World::new(true));
    let (reads, refusals) = (Arc::new(reads), Arc::new(refusals));
    for world in [&reads, &refusals] {
        world.h.remove_member("p_removed", "ws_a").await;
    }
    let limit = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    let mut ids = HashMap::new();
    let mut pending = 0;

    for row in rows {
        let name = row["op"].as_str().unwrap();
        if PENDING.contains(&name) {
            pending += 1;
            continue;
        }
        let op = find(name).unwrap_or_else(|| panic!("{} names unknown {name}", row["id"]));
        let shared = match (
            op.kind,
            row["expect"]["result"].as_str(),
            row["state"].as_str(),
        ) {
            (_, _, Some(state)) if state != "after_removal" => None,
            (Kind::Query, _, _) => Some(Arc::clone(&reads)),
            (Kind::Command, Some("deny"), _) => Some(Arc::clone(&refusals)),
            _ => None,
        };
        let limit = Arc::clone(&limit);
        let id = row["id"].as_str().unwrap().to_owned();
        let task = tasks.spawn(async move {
            let _slot = limit.acquire_owned().await.unwrap();
            let outcome = match shared {
                Some(world) => world.check(&row).await,
                None => {
                    let traffic = matches!(op.name, "recordings.save" | "recordings.unsave");
                    World::new(traffic).await.check(&row).await
                }
            };
            (row, outcome)
        });
        ids.insert(task.id(), id);
    }

    let mut failures = Vec::new();
    let mut groups: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    let mut ran: BTreeMap<&str, usize> = BTreeMap::new();
    while let Some(done) = tasks.join_next_with_id().await {
        let (row, outcome) = match done {
            Ok((_, done)) => done,
            Err(error) => {
                failures.push(format!("{} panicked: {error}", ids[&error.id()]));
                continue;
            }
        };
        *ran.entry(classes[row["principal"].as_str().unwrap()])
            .or_default() += 1;
        match outcome {
            Ok(answer) => {
                if let Some(group) = row["same_response_group"].as_str() {
                    groups.entry(group.into()).or_default().push((
                        row["id"].as_str().unwrap().into(),
                        [answer.status.as_str().as_bytes(), &answer.bytes].concat(),
                    ));
                }
            }
            Err(failure) => failures.push(failure),
        }
    }
    for (group, answers) in &groups {
        for (id, bytes) in answers {
            if *bytes != answers[0].1 {
                failures.push(format!(
                    "{id} answers unlike {} in group {group}: {} vs {}",
                    answers[0].0,
                    String::from_utf8_lossy(bytes),
                    String::from_utf8_lossy(&answers[0].1)
                ));
            }
        }
    }
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of the rows failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    println!(
        "access fixture: rows per principal class {ran:?}; {pending} pending {PENDING:?}; \
         allowed rows of {DEFERRED:?} end at the deferred handler"
    );
}

/// Milestone 10 acceptance: each protected operation of the generated registry has fixture rows
/// for every principal class (user roles, key, share, signed-out), with both an allowed and a
/// denied case, and every row names the operation's real kind.
#[test]
fn every_protected_operation_has_rows_for_every_principal_class() {
    let cases = fixture("cases.json");
    let classes = principal_classes();
    let mut seen: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    let mut problems = Vec::new();
    let mut ids = HashSet::new();
    for row in cases["cases"].as_array().unwrap() {
        let name = row["op"].as_str().unwrap();
        let id = row["id"].as_str().unwrap();
        if !ids.insert(id) {
            problems.push(format!("{id} occurs twice"));
        }
        match find(name) {
            Some(op) => {
                let kind = if op.kind == Kind::Query {
                    "query"
                } else {
                    "command"
                };
                if row["kind"] != kind {
                    problems.push(format!("{id}: {name} is a {kind}"));
                }
            }
            None if PENDING.contains(&name) => {}
            None => problems.push(format!("{id}: {name} is not in the registry")),
        }
        seen.entry(name).or_default().push((
            classes[row["principal"].as_str().unwrap()],
            row["expect"]["result"].as_str().unwrap(),
        ));
    }
    for op in operations::all().filter(|op| op.access == Access::Person) {
        let rows = seen.get(op.name).map(Vec::as_slice).unwrap_or_default();
        for wanted in ["user", "key", "share", "signed-out"] {
            if !rows.iter().any(|(class, _)| *class == wanted) {
                problems.push(format!("{} has no {wanted} row", op.name));
            }
        }
        for result in ["allow", "deny"] {
            if !rows.iter().any(|(_, r)| *r == result) {
                problems.push(format!("{} has no {result} row", op.name));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Milestone 03 acceptance: a guessed ID, a site in another workspace and a site the caller was
/// removed from all answer exactly like a missing site, for reads and writes.
#[tokio::test]
async fn hidden_sites_answer_exactly_like_missing_ones() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let removed = harness.session("p_removed").await;
    harness.remove_member("p_removed", "ws_a").await;
    let admin = harness.session("p_install_admin").await;

    let read = |cookie: String, site: &str| {
        let site = harness.sites[site].to_string();
        let harness = &harness;
        async move {
            harness
                .query(Some(&cookie), "sites.get", &format!("site={site}"))
                .await
        }
    };
    let write = |cookie: String, site: &str| {
        let site = harness.sites[site].to_string();
        let harness = &harness;
        async move {
            harness
                .command(Some(&cookie), "sites.update", json!({ "idempotency_key": site.clone(), "site": site, "name": "x", "domain": "x.example", "expected_revision": 3 }))
                .await
        }
    };

    let answers = [
        read(member.clone(), "site_guess").await,
        read(member.clone(), "site_b1").await,
        read(removed.clone(), "site_a1").await,
        read(admin.clone(), "site_a1").await,
        write(member.clone(), "site_guess").await,
        write(member.clone(), "site_b1").await,
        write(removed, "site_a1").await,
        write(admin, "site_a1").await,
    ];
    for answer in &answers {
        assert_eq!(answer.status, StatusCode::NOT_FOUND);
        assert_eq!(
            answer.bytes,
            answers[0].bytes,
            "bodies differ: {}",
            answer.json()
        );
    }
    assert_eq!(read(member, "site_a1").await.status, StatusCode::OK);
}

#[tokio::test]
async fn scope_resolution_refuses_hidden_and_missing_sites_alike() {
    let harness = Harness::new().await;
    let mut conn = harness.state.db.reader.acquire().await.unwrap();
    let member = &harness.users["p_member"];

    let missing = scope::resolve(&mut conn, member, &harness.sites["site_guess"])
        .await
        .unwrap_err();
    let foreign = scope::resolve(&mut conn, member, &harness.sites["site_b1"])
        .await
        .unwrap_err();
    assert_eq!(missing, foreign);

    let own = scope::resolve(&mut conn, member, &harness.sites["site_a1"])
        .await
        .unwrap();
    assert_eq!(own.site(), &harness.sites["site_a1"]);
}

#[tokio::test]
async fn login_sets_a_session_and_logout_ends_it() {
    let harness = Harness::new().await;
    let login = harness
        .command(None, "auth.login", json!({ "idempotency_key": "l1", "email": "P_Member@example.test", "password": PASSWORD }))
        .await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.json());
    let set_cookie = login.header("set-cookie").to_owned();
    assert!(set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Lax"));
    let cookie = set_cookie.split(';').next().unwrap().to_owned();

    let session = harness.query(Some(&cookie), "auth.session", "").await;
    assert_eq!(session.json()["account"]["email"], "p_member@example.test");

    let logout = harness
        .command(
            Some(&cookie),
            "auth.logout",
            json!({ "idempotency_key": "l2" }),
        )
        .await;
    assert_eq!(logout.status, StatusCode::OK);
    assert!(logout.header("set-cookie").contains("Max-Age=0"));
    let after = harness.query(Some(&cookie), "auth.session", "").await;
    assert_eq!(after.json()["account"], Value::Null);
    assert_eq!(
        harness
            .query(Some(&cookie), "workspaces.list", "")
            .await
            .code(),
        "unauthenticated"
    );
}

/// AX-S018: an outsider cannot tell a wrong password from an unknown account, and a wrong
/// password of an enrolled account does not reveal the second factor.
#[tokio::test]
async fn wrong_password_and_unknown_email_are_indistinguishable() {
    let world = World::new(false).await;
    let login = |email: &'static str| {
        let args = json!({ "email": email, "password": "wrong password" });
        world.send(
            Kind::Command,
            "auth.login",
            &Credential::Anonymous,
            args,
            None,
        )
    };
    let wrong = login("p_2fa_enrolled@example.test").await;
    let unknown = login("nobody@example.test").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "invalid_credentials");
    assert_eq!(wrong.bytes, unknown.bytes);
    assert!(wrong.header("set-cookie").is_empty());
}

/// AX-S023 and decision 0006: init prints a generated password once; no known default works.
#[tokio::test]
async fn init_creates_an_admin_without_a_known_default_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let data = dir.path().join("data");
    let done = init::init(InitArgs {
        data_dir: data.clone(),
        admin_email: "Admin@Example.test".into(),
        admin_password: None,
    })
    .await
    .unwrap();
    let password = done.generated_password.unwrap();
    assert_eq!(password.chars().count(), 24);
    init::check_initialized(&data).unwrap();

    let again = init::init(InitArgs {
        data_dir: data.clone(),
        admin_email: "admin@example.test".into(),
        admin_password: None,
    })
    .await;
    assert!(matches!(again, Err(failure) if failure.code == "already_initialized"));

    let db = holocron_server::db::Db::open(&data).await.unwrap();
    let mut conn = db.reader.acquire().await.unwrap();
    let (hash,): (String,) =
        sqlx::query_as("SELECT password_hash FROM users WHERE email = 'admin@example.test'")
            .fetch_one(&mut *conn)
            .await
            .unwrap();
    assert!(holocron_server::identity::verify_password(hash.clone(), password).await);
    assert!(!holocron_server::identity::verify_password(hash, "umami".into()).await);
}

/// AX-S001 to AX-S003, AX-S007, AX-S008 and AX-S024 as written: `boards.update` steps on
/// `board_a1` with idempotency keys and expected revisions, each scenario on a fresh seed.
/// `layout` is the board name; revisions are relative to the seeded revision 7.
#[tokio::test(flavor = "multi_thread")]
async fn idempotency_and_revision_scenarios_follow_the_fixture() {
    let scenarios = fixture("scenarios.json");
    let wanted = [
        "AX-S001", "AX-S002", "AX-S003", "AX-S007", "AX-S008", "AX-S024",
    ];
    let mut ran = 0;
    for scenario in scenarios["scenarios"].as_array().unwrap() {
        let id = scenario["id"].as_str().unwrap();
        if !wanted.contains(&id) {
            continue;
        }
        let world = World::new(false).await;
        let mut cookies: HashMap<String, Credential> = HashMap::new();
        let mut stored: HashMap<(String, String), Vec<u8>> = HashMap::new();
        let mut groups: HashMap<String, Vec<u8>> = HashMap::new();
        for (index, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
            let at = format!("{id} step {index}");
            let actor = step["actor"].as_str().unwrap();
            let credential = match cookies.get(actor) {
                Some(credential) => credential.clone(),
                None => {
                    let credential = Credential::Cookie(world.h.session(actor).await);
                    cookies.insert(actor.into(), credential.clone());
                    credential
                }
            };
            let row = json!({ "op": step["op"], "resource": step["args"]["board"], "args": step["args"] });
            let args = world.args(&row, actor);
            let key = step["idempotency_key"].as_str();
            let answer = world
                .send(
                    Kind::Command,
                    step["op"].as_str().unwrap(),
                    &credential,
                    args,
                    key,
                )
                .await;
            let expect = &step["expect"];
            match expect["result"].as_str().unwrap() {
                "allow" => {
                    assert_eq!(answer.status, StatusCode::OK, "{at}: {}", answer.json());
                    if let Some(revision) = expect["revision"].as_i64() {
                        assert_eq!(answer.json()["revision"], world.revision(revision), "{at}");
                        assert_eq!(world.board_head().await, world.revision(revision), "{at}");
                    }
                }
                _ => {
                    assert_eq!(
                        (answer.status.as_u16() as u64, answer.code()),
                        (
                            expect["status"].as_u64().unwrap(),
                            expect["code"].as_str().unwrap().to_owned()
                        ),
                        "{at}: {}",
                        answer.json()
                    );
                }
            }
            if let Some(includes) = expect["body_includes"].as_str() {
                let current: i64 = includes
                    .strip_prefix("current_revision ")
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(answer.json()["current"], world.revision(current), "{at}");
            }
            if let Some(key) = key {
                let entry = (actor.to_owned(), key.to_owned());
                if expect["replayed"] == true {
                    assert_eq!(
                        Some(&answer.bytes),
                        stored.get(&entry),
                        "{at}: not replayed"
                    );
                } else if answer.status == StatusCode::OK {
                    stored.insert(entry, answer.bytes.clone());
                }
            }
            if let Some(group) = step["same_response_group"].as_str() {
                let first = groups.entry(group.into()).or_insert(answer.bytes.clone());
                assert_eq!(*first, answer.bytes, "{at}: unlike group {group}");
            }
        }
        ran += 1;
    }
    assert_eq!(ran, wanted.len());
}

/// AX-S014: revoking a key ends its live stream, and reconnecting with the last cursor answers
/// like an unknown key.
#[tokio::test]
async fn revoking_a_key_ends_its_live_stream() {
    let world = World::new(false).await;
    let site = world.site("site_a1");
    let key = world.credentials["p_key_site"].clone();
    let stream = world.h.subscribe(("authorization", &key), &site).await;
    assert_eq!(stream.status(), StatusCode::OK);
    let reader = tokio::spawn(frames(stream.into_body(), 2));
    let member = world.h.session("p_member").await;
    world
        .run(
            &member,
            "apiKeys.revoke",
            json!({ "key": world.id("key_site") }),
        )
        .await;
    let got = reader.await.unwrap();
    assert_eq!(got[1]["kind"], "access_revoked");

    let cursor = got[0]["cursor"].as_str().unwrap();
    let resume = |authorization: String| {
        let request = Request::get(format!(
            "/api/subscribe?topic=analytics%3A{site}&cursor={}",
            encode(cursor)
        ))
        .header(header::HOST, HOST)
        .header(header::AUTHORIZATION, authorization)
        .body(Body::empty())
        .unwrap();
        world.h.send(request)
    };
    let again = resume(key).await;
    let unknown = resume("Bearer unknown".into()).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
    assert_eq!(again.code(), "unauthenticated");
    assert_eq!(again.bytes, unknown.bytes);
}

/// AX-S016, first half: a downgrade that keeps `site.read` keeps the stream delivering. The
/// second half (`operations.status`) has no topic yet.
#[tokio::test(flavor = "multi_thread")]
async fn a_downgrade_that_keeps_reading_keeps_the_stream() {
    let world = World::new(false).await;
    let site = world.site("site_a1");
    let member = world.h.session("p_member").await;
    let stream = world.h.subscribe(("cookie", &member), &site).await;
    assert_eq!(stream.status(), StatusCode::OK);
    let reader = tokio::spawn(frames(stream.into_body(), 2));
    let owner = world.h.session("p_owner").await;
    let viewer = json!({ "workspace": world.workspace("ws_a"), "user": world.user("p_member"), "role": "viewer" });
    world.run(&owner, "members.updateRole", viewer).await;
    let hit = json!({ "type": "event", "payload": { "website": site, "url": "/", "hostname": "shop.test" } });
    ok(world
        .h
        .collect("POST", "/api/send", "198.51.100.50:1", Some(hit))
        .await);
    let got = reader.await.unwrap();
    assert_eq!(
        (got[0]["kind"].as_str(), got[1]["kind"].as_str()),
        (Some("subscribed"), Some("events"))
    );
}

/// AX-S026, the parts boards, shares and live streams own: erasing a member ends their stream,
/// keeps the board and the shares they created, which stay readable and manageable, and
/// leaves no trace of their own exploration. Accounts, sessions and keys are in `accounts.rs`.
#[tokio::test]
async fn erasing_a_member_keeps_shared_work() {
    let world = World::new(false).await;
    let site = world.site("site_a1");
    let member = world.h.session("p_member").await;
    let stream = world.h.subscribe(("cookie", &member), &site).await;
    assert_eq!(stream.status(), StatusCode::OK);
    let reader = tokio::spawn(frames(stream.into_body(), 2));
    let install_admin = world.h.session("p_install_admin").await;
    world
        .run(
            &install_admin,
            "users.delete",
            json!({ "user": world.user("p_member") }),
        )
        .await;
    assert_eq!(reader.await.unwrap()[1]["kind"], "access_revoked");

    let admin = Credential::Cookie(world.h.session("p_admin").await);
    let board = world
        .send(
            Kind::Query,
            "boards.get",
            &admin,
            json!({ "board": world.id("board_a1") }),
            None,
        )
        .await;
    assert_eq!(ok(board)["board"]["cards"].as_array().unwrap().len(), 2);
    let exploration = json!({ "site": site, "exploration": world.id("exp_a1") });
    let gone = world
        .send(Kind::Query, "explorations.get", &admin, exploration, None)
        .await;
    assert_eq!(gone.code(), "not_found");
    let viewer = Credential::Header(world.credentials["p_share_viewer"].clone());
    ok(world
        .send(Kind::Query, "sharing.resolve", &viewer, json!({}), None)
        .await);
    let revoke = json!({ "share": world.id("share_a1") });
    ok(world
        .send(Kind::Command, "sharing.revoke", &admin, revoke, None)
        .await);
}
