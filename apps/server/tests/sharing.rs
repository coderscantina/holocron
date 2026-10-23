//! Milestone 10 restricted sharing over HTTP: creating and resolving shares, the widening check
//! on reads, exports and value suggestions, board shares, and revocation of reads and live
//! streams, against the `share_*` rows of `fixtures/access`.

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use common::{Answer, HOST, Harness, ORIGIN, exploration_share, frames, share_state};
use holocron_server::db::now_ms;
use serde_json::{Value, json};

const DAY_MS: i64 = 86_400_000;

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn ok(answer: &Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn explore(h: &Harness, share: &str, state: &Value) -> Answer {
    h.query_with(
        share,
        "analytics.explore",
        &format!("state={}", encode(&state.to_string())),
    )
    .await
}

async fn command(h: &Harness, cookie: &str, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    h.command(Some(cookie), name, body).await
}

fn create_args(target: Value) -> Value {
    json!({ "name": "Mobile pages", "target": target, "export": false, "recordings": false,
            "expires_at": now_ms() + 30 * DAY_MS })
}

/// `share_a1`: the pinned state of `exp_a1` on `site_a1`, 30 days, read only.
async fn share_a1(h: &Harness, export: bool) -> (String, Value) {
    let state = share_state(h.sites["site_a1"].as_str(), 30);
    let share = h
        .share(
            "ws_a",
            exploration_share(&state, 30),
            export,
            false,
            now_ms() + 30 * DAY_MS,
        )
        .await;
    (share, state)
}

#[tokio::test]
async fn creating_a_share_discloses_its_token_once_and_the_viewer_resolves_it() {
    let h = Harness::new().await;
    let member = h.session("p_member").await;
    let site = h.sites["site_a1"].to_string();
    let mut body = create_args(exploration_share(&share_state(&site, 30), 30));
    body["idempotency_key"] = json!("k1");
    let created = ok(&h
        .command(Some(&member), "sharing.create", body.clone())
        .await);
    let token = created["secret"].as_str().unwrap().to_owned();
    assert_eq!(created["share"]["prefix"], token[..8]);

    let replay = ok(&h.command(Some(&member), "sharing.create", body).await);
    assert_eq!(replay["share"]["id"], created["share"]["id"]);
    assert_eq!(
        (replay["secret"].clone(), replay["secret_withheld"].clone()),
        (Value::Null, json!(true))
    );

    let view = ok(&h
        .query_with(&format!("Share {token}"), "sharing.resolve", "")
        .await);
    assert_eq!(view["name"], "Mobile pages");
    assert_eq!(view["sites"][0]["id"], site);
    assert_eq!(view["sites"][0]["timezone"], "Europe/Vienna");
    assert!(view.get("created_by").is_none() && view.get("prefix").is_none());

    let listed = ok(&h
        .query(Some(&member), "sharing.list", &format!("site={site}"))
        .await);
    assert_eq!(listed["shares"][0]["id"], created["share"]["id"]);
}

/// AX-T022 to AX-T035 and AX-T038 over HTTP; `sharing::limits` tests every widening.
#[tokio::test]
async fn a_viewer_reads_inside_the_share_and_is_refused_outside_it() {
    let h = Harness::new().await;
    let (share, state) = share_a1(&h, false).await;

    ok(&explore(&h, &share, &state).await);
    let mut narrowed = state.clone();
    narrowed["filter"]["items"].as_array_mut().unwrap().push(
        json!({ "kind": "cond", "id": "c2", "field": "country", "cmp": "eq", "value": { "t": "string", "v": "AT" } }),
    );
    ok(&explore(&h, &share, &narrowed).await);

    let mut longer = state.clone();
    longer["range"]["count"] = json!(90);
    let mut unfiltered = state.clone();
    unfiltered["filter"] = Value::Null;
    for widened in [longer, unfiltered] {
        let answer = explore(&h, &share, &widened).await;
        assert_eq!(
            (answer.status.as_u16(), answer.code()),
            (403, "share_scope_exceeded".into())
        );
    }

    let mut elsewhere = state.clone();
    elsewhere["site"] = json!(h.sites["site_a2"]);
    assert_eq!(explore(&h, &share, &elsewhere).await.code(), "not_found");
    let export = h
        .query_with(
            &share,
            "analytics.export",
            &format!("state={}&format=csv", encode(&state.to_string())),
        )
        .await;
    assert_eq!(export.code(), "not_found");

    let site = h.sites["site_a1"].to_string();
    let values = |field: Value| {
        let query = format!("site={site}&field={}", encode(&field.to_string()));
        let h = &h;
        let share = share.clone();
        async move { h.query_with(&share, "fields.values", &query).await }
    };
    ok(&values(json!("country")).await);
    let property = values(json!({ "scope": "event", "key": "email" })).await;
    assert_eq!(property.code(), "share_scope_exceeded");
}

#[tokio::test]
async fn exports_keep_to_the_share_limits() {
    let h = Harness::new().await;
    let (share, state) = share_a1(&h, true).await;
    let export = |state: Value| {
        let query = format!("state={}&format=csv", encode(&state.to_string()));
        let h = &h;
        let share = share.clone();
        async move { h.query_with(&share, "analytics.export", &query).await }
    };
    ok(&export(state.clone()).await);
    let mut longer = state;
    longer["range"]["count"] = json!(90);
    assert_eq!(export(longer).await.code(), "share_scope_exceeded");
}

/// AX-S010, AX-S015 and the `share-token` group (AX-T041 to AX-T043).
#[tokio::test]
async fn revoking_a_share_ends_reads_and_the_live_stream() {
    let h = Harness::new().await;
    let member = h.session("p_member").await;
    let site = h.sites["site_a1"].to_string();
    let state = share_state(&site, 30);
    let created = ok(&command(
        &h,
        &member,
        "sharing.create",
        create_args(exploration_share(&state, 30)),
    )
    .await);
    let share = format!("Share {}", created["secret"].as_str().unwrap());

    let stream = h.subscribe(("authorization", &share), &site).await;
    assert_eq!(stream.status(), 200);
    let reader = tokio::spawn(frames(stream.into_body(), 2));
    ok(&explore(&h, &share, &state).await);
    ok(&command(
        &h,
        &member,
        "sharing.revoke",
        json!({ "share": created["share"]["id"] }),
    )
    .await);

    let got = reader.await.unwrap();
    assert_eq!(got[0]["kind"], "subscribed");
    assert_eq!(got[1]["kind"], "access_revoked");
    assert_eq!(explore(&h, &share, &state).await.code(), "not_found");
    assert_eq!(
        h.subscribe(("authorization", &share), &site).await.status(),
        404
    );

    let expired = h
        .share(
            "ws_a",
            exploration_share(&state, 30),
            false,
            false,
            now_ms() - 1,
        )
        .await;
    let mut answers = Vec::new();
    for token in [share, expired, "Share unknown".into()] {
        let answer = h.query_with(&token, "sharing.resolve", "").await;
        answers.push((answer.status.as_u16(), answer.bytes));
    }
    assert_eq!(answers[0].0, 404);
    assert!(answers.iter().all(|answer| *answer == answers[0]));
}

/// AX-T012 and AX-T045 to AX-T047.
#[tokio::test]
async fn a_board_share_reads_its_board_with_other_sites_unavailable() {
    let h = Harness::new().await;
    let member = h.session("p_member").await;
    let board = ok(&command(
        &h,
        &member,
        "boards.create",
        json!({ "workspace": h.workspaces["ws_a"], "name": "Overview" }),
    )
    .await)["board"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let a1 = share_state(h.sites["site_a1"].as_str(), 30);
    let a2 = share_state(h.sites["site_a2"].as_str(), 30);
    for state in [&a1, &a2] {
        ok(&command(
            &h,
            &member,
            "boards.pin",
            json!({ "board": board, "title": "Mobile", "state": state }),
        )
        .await);
    }
    let target = json!({ "kind": "board", "board": board, "sites": [h.sites["site_a1"]] });
    let share = h
        .share("ws_a", target, false, false, now_ms() + DAY_MS)
        .await;

    let read = ok(&h
        .query_with(&share, "boards.get", &format!("board={board}"))
        .await);
    let cards = read["board"]["cards"].as_array().unwrap();
    assert_eq!(cards[0]["state"], a1);
    assert!(cards[1].get("state").is_none() && cards[1].get("exploration").is_none());
    assert_eq!(read["board"]["can_edit"], false);

    let mut week = a1.clone();
    week["range"]["count"] = json!(7);
    ok(&explore(&h, &share, &week).await);
    let mut other = a1.clone();
    other["dimensions"] = json!(["city"]);
    assert_eq!(
        explore(&h, &share, &other).await.code(),
        "share_scope_exceeded"
    );
    assert_eq!(explore(&h, &share, &a2).await.code(), "not_found");
    let body = json!({ "idempotency_key": "k", "board": board, "name": "x", "cards": [] });
    let update = h
        .send(
            Request::post("/api/commands/boards.update")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header("x-holocron-request", "1")
                .header(header::AUTHORIZATION, &share)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(update.code(), "not_found");

    let (exploration_share, _) = share_a1(&h, false).await;
    let answer = h
        .query_with(&exploration_share, "boards.get", &format!("board={board}"))
        .await;
    assert_eq!(answer.code(), "not_found");
}

/// A share never grants more than its creator holds or its state reads, and hidden shares
/// answer like missing ones (AX-T017, AX-T018).
#[tokio::test]
async fn shares_stay_inside_the_creator_and_the_state() {
    let h = Harness::new().await;
    let state = share_state(h.sites["site_a1"].as_str(), 30);
    let viewer = h.session("p_viewer").await;
    let outsider = h.session("p_other_owner").await;
    let member = h.session("p_member").await;
    let create = |cookie: String, args: Value| {
        let h = &h;
        async move { command(h, &cookie, "sharing.create", args).await.code() }
    };
    let args = create_args(exploration_share(&state, 30));
    assert_eq!(create(viewer, args.clone()).await, "forbidden");
    assert_eq!(create(outsider.clone(), args.clone()).await, "not_found");

    let mut wider = args.clone();
    wider["target"]["limits"]["dimensions"] = json!(["path", "country", "city"]);
    let mut endless = args.clone();
    endless["expires_at"] = json!(now_ms() + 367 * DAY_MS);
    for invalid in [wider, endless] {
        assert_eq!(create(member.clone(), invalid).await, "invalid_arguments");
    }

    let created = ok(&command(&h, &member, "sharing.create", args).await);
    let hidden = command(
        &h,
        &outsider,
        "sharing.revoke",
        json!({ "share": created["share"]["id"] }),
    )
    .await;
    let missing = command(
        &h,
        &outsider,
        "sharing.revoke",
        json!({ "share": uuid::Uuid::new_v4() }),
    )
    .await;
    assert_eq!(
        (hidden.status, hidden.bytes),
        (missing.status, missing.bytes)
    );
}
