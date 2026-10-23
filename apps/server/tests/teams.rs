//! Milestone 10 teams, members, invitations and site lifecycle over HTTP, against the `teams.*`,
//! `members.*` and `sites.*` rows of `fixtures/access` and its revocation scenarios.

mod common;

use common::{Answer, Harness, exploration_share, frames, share_state};
use holocron_server::db::now_ms;
use serde_json::{Value, json};

const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

fn ok(answer: &Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn command(h: &Harness, cookie: &str, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    h.command(Some(cookie), name, body).await
}

async fn code(h: &Harness, cookie: &str, name: &str, body: Value) -> String {
    let answer = command(h, cookie, name, body).await;
    if answer.status == 200 {
        "ok".into()
    } else {
        answer.code()
    }
}

async fn explore(
    h: &Harness,
    credential: Option<&str>,
    cookie: Option<&str>,
    site: &str,
) -> Answer {
    let state = share_state(site, 30).to_string();
    let query = format!(
        "state={}",
        url::form_urlencoded::byte_serialize(state.as_bytes()).collect::<String>()
    );
    match credential {
        Some(authorization) => {
            h.query_with(authorization, "analytics.explore", &query)
                .await
        }
        None => h.query(cookie, "analytics.explore", &query).await,
    }
}

#[tokio::test]
async fn invitations_are_redeemed_once_with_their_role() {
    let h = Harness::new().await;
    let admin = h.session("p_admin").await;
    let workspace = h.workspaces["ws_a"].to_string();
    let invite = |cookie: String, role: &'static str| {
        let h = &h;
        let workspace = workspace.clone();
        async move {
            command(
                h,
                &cookie,
                "teams.invite",
                json!({ "workspace": workspace, "role": role, "label": "Ana" }),
            )
            .await
        }
    };
    assert_eq!(invite(admin.clone(), "owner").await.code(), "forbidden");
    assert_eq!(
        invite(h.session("p_member").await, "viewer").await.code(),
        "forbidden"
    );
    let created = ok(&invite(admin.clone(), "member").await);
    let code = created["secret"].as_str().unwrap().to_owned();
    assert_eq!(created["invitation"]["state"], "pending");

    let joiner = h.session("p_other_owner").await;
    let joined = ok(&command(&h, &joiner, "teams.join", json!({ "code": code })).await);
    assert_eq!(
        (
            joined["team"]["role"].clone(),
            joined["team"]["members"].clone()
        ),
        (json!("member"), json!(9))
    );

    let again = command(
        &h,
        &h.session("p_install_admin").await,
        "teams.join",
        json!({ "code": code }),
    )
    .await;
    let unknown = command(
        &h,
        &h.session("p_install_admin").await,
        "teams.join",
        json!({ "code": "nope" }),
    )
    .await;
    assert_eq!(again.code(), "not_found");
    assert_eq!(again.bytes, unknown.bytes);
    let listed = ok(&h
        .query(
            Some(&admin),
            "teams.invitations",
            &format!("workspace={workspace}"),
        )
        .await);
    assert_eq!(listed["invitations"][0]["state"], "used");
}

/// AX-T048 to AX-T050, AX-T102, AX-T104 and AX-T123.
#[tokio::test]
async fn role_changes_follow_rank_and_reauthentication() {
    let h = Harness::new().await;
    let workspace = h.workspaces["ws_a"].to_string();
    let change = |user: &str, role: &str| json!({ "workspace": workspace, "user": h.users[user], "role": role });
    let admin = h.session_reauthenticated("p_admin", None).await;
    let owner = h.session_reauthenticated("p_owner", None).await;

    assert_eq!(
        code(
            &h,
            &admin,
            "members.updateRole",
            change("p_member", "owner")
        )
        .await,
        "forbidden"
    );
    assert_eq!(
        code(
            &h,
            &admin,
            "members.updateRole",
            change("p_owner", "viewer")
        )
        .await,
        "forbidden"
    );
    assert_eq!(
        code(
            &h,
            &admin,
            "members.updateRole",
            change("p_member", "viewer")
        )
        .await,
        "ok"
    );
    assert_eq!(
        code(
            &h,
            &owner,
            "members.updateRole",
            change("p_viewer", "admin")
        )
        .await,
        "reauthentication_required"
    );
    h.set_reauthenticated_at(&owner, Some(now_ms() - 9 * MINUTE_MS))
        .await;
    assert_eq!(
        code(
            &h,
            &owner,
            "members.updateRole",
            change("p_viewer", "admin")
        )
        .await,
        "ok"
    );
    assert_eq!(
        code(
            &h,
            &owner,
            "members.updateRole",
            change("p_admin", "member")
        )
        .await,
        "ok"
    );

    let team = ok(&h
        .query(Some(&owner), "teams.get", &format!("workspace={workspace}"))
        .await);
    let roles: Vec<&str> = team["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["role"].as_str().unwrap())
        .collect();
    assert_eq!(roles[..2], ["owner", "admin"]);
}

/// AX-S028: an owner demotes another owner, and the one left can neither demote themselves nor
/// leave, so the team keeps an owner.
#[tokio::test]
async fn owners_demote_other_owners_and_the_team_keeps_one() {
    let h = Harness::new().await;
    let workspace = h.workspaces["ws_a"].to_string();
    let change = |user: &str, role: &str| json!({ "workspace": workspace, "user": h.users[user], "role": role });
    let owner = h.session("p_owner").await;
    let admin = h.session_reauthenticated("p_admin", None).await;

    let steps = [
        (
            &owner,
            "members.updateRole",
            change("p_admin", "owner"),
            "ok",
        ),
        (
            &admin,
            "members.updateRole",
            change("p_owner", "member"),
            "ok",
        ),
        (
            &admin,
            "members.updateRole",
            change("p_admin", "admin"),
            "forbidden",
        ),
        (
            &admin,
            "members.remove",
            json!({ "workspace": workspace, "user": h.users["p_admin"] }),
            "invalid_arguments",
        ),
        (
            &owner,
            "members.updateRole",
            change("p_admin", "member"),
            "forbidden",
        ),
    ];
    for (i, (cookie, name, body, expected)) in steps.into_iter().enumerate() {
        assert_eq!(
            code(&h, cookie, name, body).await,
            expected,
            "step {}",
            i + 1
        );
    }
}

/// AX-T051 to AX-T053, the last owner, and AX-S011 / AX-S012: removal cuts the member and
/// their keys off at the next request.
#[tokio::test]
async fn members_leave_or_are_removed_by_rank_and_lose_access_at_once() {
    let h = Harness::new().await;
    let workspace = h.workspaces["ws_a"].to_string();
    let remove = |user: &str| json!({ "workspace": workspace, "user": h.users[user] });
    let site = h.sites["site_a1"].to_string();

    assert_eq!(
        code(
            &h,
            &h.session("p_admin").await,
            "members.remove",
            remove("p_owner")
        )
        .await,
        "forbidden"
    );
    let member = h.session("p_member").await;
    assert_eq!(
        code(&h, &member, "members.remove", remove("p_viewer")).await,
        "forbidden"
    );
    let owner = h.session("p_owner").await;
    assert_eq!(
        code(&h, &owner, "members.remove", remove("p_owner")).await,
        "invalid_arguments"
    );

    let key = h
        .api_key("p_member", &["analytics.explore"], Some(&["site_a1"]))
        .await;
    ok(&explore(&h, Some(&key), None, &site).await);
    assert_eq!(
        code(&h, &member, "members.remove", remove("p_member")).await,
        "ok"
    );
    let gone = explore(&h, None, Some(&member), &site).await;
    let guessed = explore(&h, None, Some(&member), h.sites["site_guess"].as_str()).await;
    assert_eq!((gone.code(), gone.bytes), (guessed.code(), guessed.bytes));
    assert_eq!(
        explore(&h, Some(&key), None, &site).await.code(),
        "not_found"
    );
}

/// AX-S013: removing a member ends their live stream, and reconnecting is `not_found`.
#[tokio::test]
async fn removing_a_member_ends_their_live_stream() {
    let h = Harness::new().await;
    let removed = h.session("p_removed").await;
    let site = h.sites["site_a1"].to_string();
    let stream = h.subscribe(("cookie", &removed), &site).await;
    assert_eq!(stream.status(), 200);
    let reader = tokio::spawn(frames(stream.into_body(), 2));
    let admin = h.session("p_admin").await;
    let body = json!({ "workspace": h.workspaces["ws_a"], "user": h.users["p_removed"] });
    ok(&command(&h, &admin, "members.remove", body).await);
    assert_eq!(reader.await.unwrap()[1]["kind"], "access_revoked");
    assert_eq!(h.subscribe(("cookie", &removed), &site).await.status(), 404);
}

/// AX-S004 and AX-S005: a stored board answer is authorized again before it is replayed.
#[tokio::test]
async fn board_replays_are_authorized_again() {
    let h = Harness::new().await;
    let member = h.session("p_member").await;
    let workspace = h.workspaces["ws_a"].to_string();
    let board = ok(&command(
        &h,
        &member,
        "boards.create",
        json!({ "workspace": workspace, "name": "B" }),
    )
    .await)["board"]["id"]
        .clone();
    let update = json!({ "idempotency_key": "K5", "board": board, "name": "L1", "cards": [] });
    ok(&h
        .command(Some(&member), "boards.update", update.clone())
        .await);
    let owner = h.session("p_owner").await;
    let viewer = json!({ "workspace": workspace, "user": h.users["p_member"], "role": "viewer" });
    ok(&command(&h, &owner, "members.updateRole", viewer).await);
    assert_eq!(
        h.command(Some(&member), "boards.update", update.clone())
            .await
            .code(),
        "forbidden"
    );
    h.remove_member("p_member", "ws_a").await;
    assert_eq!(
        h.command(Some(&member), "boards.update", update)
            .await
            .code(),
        "not_found"
    );
}

/// AX-T054, AX-T114 and AX-T116.
#[tokio::test]
async fn deleting_a_team_is_for_reauthenticated_owners() {
    let h = Harness::new().await;
    let workspace = json!({ "workspace": h.workspaces["ws_a"] });
    assert_eq!(
        code(
            &h,
            &h.session("p_admin").await,
            "teams.delete",
            workspace.clone()
        )
        .await,
        "forbidden"
    );
    let owner = h.session_reauthenticated("p_owner", None).await;
    assert_eq!(
        code(&h, &owner, "teams.delete", workspace.clone()).await,
        "reauthentication_required"
    );
    h.set_reauthenticated_at(&owner, Some(now_ms())).await;
    assert_eq!(code(&h, &owner, "teams.delete", workspace).await, "ok");
    let site = h.sites["site_a1"].to_string();
    let read = h
        .query(Some(&owner), "sites.get", &format!("site={site}"))
        .await;
    assert_eq!(read.code(), "not_found");
}

#[tokio::test]
async fn teams_and_sites_are_listed_by_membership_and_for_install_admins() {
    let h = Harness::new().await;
    let created = ok(&command(
        &h,
        &h.session("p_member").await,
        "teams.create",
        json!({ "name": "Side project" }),
    )
    .await);
    assert_eq!(created["team"]["role"], "owner");
    let teams = ok(&h
        .query(Some(&h.session("p_member").await), "teams.list", "")
        .await);
    let names: Vec<&str> = teams["teams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Acme", "Side project"]);

    let admin = h.session("p_install_admin").await;
    let teams = ok(&h.query(Some(&admin), "teams.list", "").await);
    assert_eq!(teams["teams"].as_array().unwrap().len(), 3);
    assert!(teams["teams"][0].get("role").is_none());
    let sites = ok(&h.query(Some(&admin), "sites.list", "").await);
    assert_eq!(sites["sites"].as_array().unwrap().len(), 3);
    assert!(sites["sites"][0].get("role").is_none());
    let member_sites = ok(&h
        .query(Some(&h.session("p_viewer").await), "sites.list", "")
        .await);
    assert_eq!(member_sites["sites"].as_array().unwrap().len(), 2);
}

/// `sites.create`, and AX-T083, AX-T105, AX-T107, AX-T108, AX-T125 to AX-T128.
#[tokio::test]
async fn site_lifecycle_follows_capabilities_and_reauthentication() {
    let h = Harness::new().await;
    let member = h.session("p_member").await;
    let args = json!({ "workspace": h.workspaces["ws_a"], "name": "Shop", "domain": "Shop.Example", "timezone": "Europe/Vienna" });
    let created = ok(&command(&h, &member, "sites.create", args).await);
    assert_eq!(created["site"]["domain"], "shop.example");
    let site = |alias: &str| json!({ "site": h.sites[alias] });

    assert_eq!(
        code(&h, &member, "sites.delete", site("site_a1")).await,
        "forbidden"
    );
    assert_eq!(
        code(&h, &member, "sites.reset", site("site_a1")).await,
        "forbidden"
    );
    let viewer = h.session_reauthenticated("p_viewer", None).await;
    assert_eq!(
        code(&h, &viewer, "sites.delete", site("site_a1")).await,
        "forbidden"
    );
    let outsider = h.session_reauthenticated("p_other_owner", None).await;
    let hidden = command(&h, &outsider, "sites.delete", site("site_a1")).await;
    let guessed = command(&h, &outsider, "sites.delete", site("site_guess")).await;
    assert_eq!(
        (hidden.code(), hidden.bytes),
        (guessed.code(), guessed.bytes)
    );

    let admin = h.session_reauthenticated("p_admin", None).await;
    assert_eq!(
        code(&h, &admin, "sites.delete", site("site_a1")).await,
        "reauthentication_required"
    );
    assert_eq!(
        code(&h, &admin, "sites.reset", site("site_a1")).await,
        "reauthentication_required"
    );
    h.set_reauthenticated_at(&admin, Some(now_ms() - 9 * MINUTE_MS))
        .await;
    assert_eq!(
        code(&h, &admin, "sites.delete", site("site_a1")).await,
        "ok"
    );
    let read = h
        .query(
            Some(&admin),
            "sites.get",
            &format!("site={}", h.sites["site_a1"]),
        )
        .await;
    assert_eq!(read.code(), "not_found");
}

/// AX-T056, AX-T111 and AX-T113: a transfer needs the target and a fresh login, and ends the
/// site's shares and key grants.
#[tokio::test]
async fn transferring_a_site_ends_its_shares_and_key_grants() {
    let h = Harness::new().await;
    let site = h.sites["site_a1"].to_string();
    let admin = h.session("p_admin").await;
    let to_team =
        json!({ "site": site, "to": { "direction": "to_team", "team": h.workspaces["ws_b"] } });
    assert_eq!(
        code(&h, &admin, "sites.transfer", to_team).await,
        "not_found"
    );
    let to_owner =
        json!({ "site": site, "to": { "direction": "to_user", "user": h.users["p_owner"] } });
    assert_eq!(
        code(&h, &admin, "sites.transfer", to_owner.clone()).await,
        "forbidden"
    );

    let share = h
        .share(
            "ws_a",
            exploration_share(&share_state(&site, 30), 30),
            false,
            false,
            now_ms() + DAY_MS,
        )
        .await;
    let key = h
        .api_key("p_owner", &["analytics.explore"], Some(&["site_a1"]))
        .await;
    let owner = h.session_reauthenticated("p_owner", None).await;
    assert_eq!(
        code(&h, &owner, "sites.transfer", to_owner.clone()).await,
        "reauthentication_required"
    );
    h.set_reauthenticated_at(&owner, Some(now_ms())).await;
    let moved = ok(&command(&h, &owner, "sites.transfer", to_owner).await);
    assert_ne!(moved["site"]["workspace"], json!(h.workspaces["ws_a"]));
    assert!(
        moved["site"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("site.transfer"))
    );

    assert_eq!(
        explore(&h, Some(&share), None, &site).await.code(),
        "not_found"
    );
    assert_eq!(
        explore(&h, Some(&key), None, &site).await.code(),
        "not_found"
    );
    ok(&explore(&h, None, Some(&owner), &site).await);
    let admins_view = explore(&h, None, Some(&admin), &site).await;
    assert_eq!(admins_view.code(), "not_found");
}
