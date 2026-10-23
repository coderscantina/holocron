//! Milestone 10 identity over HTTP: second factors, reauthentication, sessions, scoped keys,
//! security policy and user erasure, against `fixtures/access` (AX-S006 to AX-S027).

mod common;

use axum::http::StatusCode;
use common::{Answer, Harness, PASSWORD};
use holocron_protocol::limits::REAUTHENTICATION_WINDOW_MS;
use holocron_server::db::now_ms;
use holocron_server::identity::{SECRETS_KEY_FILE, totp};
use holocron_server::init::{self, InitArgs};
use serde_json::{Value, json};

const NEW_PASSWORD: &str = "a much longer new password";

/// A command with a fresh idempotency key.
async fn command(harness: &Harness, cookie: Option<&str>, name: &str, mut body: Value) -> Answer {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    harness.command(cookie, name, body).await
}

/// A command that must succeed, returning its answer.
async fn ok(harness: &Harness, cookie: &str, name: &str, body: Value) -> Value {
    let answer = command(harness, Some(cookie), name, body).await;
    assert_eq!(answer.status, StatusCode::OK, "{name}: {}", answer.json());
    answer.json()
}

/// `name=value` of a cookie the answer sets, unless it clears it.
fn set_cookie(answer: &Answer, name: &str) -> Option<String> {
    answer
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .find(|pair| pair.starts_with(&format!("{name}=")) && pair.len() > name.len() + 1)
        .map(str::to_owned)
}

async fn login(harness: &Harness, alias: &str, password: &str) -> Answer {
    command(
        harness,
        None,
        "auth.login",
        json!({ "email": format!("{alias}@example.test"), "password": password }),
    )
    .await
}

async fn site_get(harness: &Harness, cookie: &str) -> Answer {
    let site = &harness.sites["site_a1"];
    harness
        .query(Some(cookie), "sites.get", &format!("site={site}"))
        .await
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

/// A code one step ahead, which stays acceptable after the current step was spent.
fn next_code(secret: &[u8]) -> String {
    totp::code(secret, totp::step(now_ms()) + 1)
}

/// Enrolls the person behind `cookie` through the API. Returns the TOTP secret and the
/// recovery codes.
async fn enroll(harness: &Harness, cookie: &str) -> (Vec<u8>, Vec<String>) {
    let begin = ok(harness, cookie, "twoFactor.begin", json!({})).await;
    let secret = base32_decode(begin["secret"]["key"].as_str().unwrap());
    let code = totp::code(&secret, totp::step(now_ms()));
    let confirm = ok(
        harness,
        cookie,
        "twoFactor.confirm",
        json!({ "code": code }),
    )
    .await;
    let codes = confirm["secret"]
        .as_array()
        .unwrap()
        .iter()
        .map(|code| code.as_str().unwrap().to_owned())
        .collect();
    (secret, codes)
}

/// Turns on the 2FA requirement of `ws_a` as its owner.
async fn require_two_factor_in_ws_a(harness: &Harness) {
    let owner = harness.session("p_owner").await;
    let workspace = &harness.workspaces["ws_a"];
    ok(
        harness,
        &owner,
        "security.updatePolicy",
        json!({ "target": { "scope": "team", "workspace": workspace }, "two_factor_required": true }),
    )
    .await;
}

/// AX-S017: a correct password of an enrolled account yields a challenge, never a session, and
/// the second factor turns it into one. Codes are accepted once.
#[tokio::test]
async fn enrolled_login_needs_the_second_factor() {
    let harness = Harness::new().await;
    let (secret, _) = enroll(&harness, &harness.session("p_2fa_enrolled").await).await;

    let password = login(&harness, "p_2fa_enrolled", PASSWORD).await;
    assert_eq!(password.json()["status"], "second_factor_required");
    assert!(set_cookie(&password, "holocron_session").is_none());
    let challenge = set_cookie(&password, "holocron_challenge").unwrap();
    assert_eq!(
        site_get(&harness, &challenge).await.code(),
        "unauthenticated"
    );

    let wrong = command(
        &harness,
        Some(&challenge),
        "auth.verifySecondFactor",
        json!({ "kind": "totp", "code": "000000" }),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.code(), "second_factor_invalid");

    let code = next_code(&secret);
    let verified = command(
        &harness,
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
    let session = set_cookie(&verified, "holocron_session").unwrap();
    assert_eq!(site_get(&harness, &session).await.status, StatusCode::OK);

    let again = login(&harness, "p_2fa_enrolled", PASSWORD).await;
    let challenge = set_cookie(&again, "holocron_challenge").unwrap();
    let replayed = command(
        &harness,
        Some(&challenge),
        "auth.verifySecondFactor",
        json!({ "kind": "totp", "code": code }),
    )
    .await;
    assert_eq!(replayed.code(), "second_factor_invalid");
}

/// Decision 0012: after 5 wrong codes the challenge ends, even for a right one. Guesses sent
/// at the same time cannot pass the limit.
#[tokio::test(flavor = "multi_thread")]
async fn a_challenge_ends_after_five_wrong_codes() {
    let harness = std::sync::Arc::new(Harness::new().await);
    let (secret, _) = enroll(&harness, &harness.session("p_2fa_enrolled").await).await;
    let challenge = set_cookie(
        &login(&harness, "p_2fa_enrolled", PASSWORD).await,
        "holocron_challenge",
    )
    .unwrap();

    let guesses: Vec<_> = (0..8)
        .map(|_| {
            let harness = std::sync::Arc::clone(&harness);
            let challenge = challenge.clone();
            tokio::spawn(async move {
                let wrong = json!({ "kind": "totp", "code": "000000" });
                command(&harness, Some(&challenge), "auth.verifySecondFactor", wrong)
                    .await
                    .code()
            })
        })
        .collect();
    let mut codes = Vec::new();
    for guess in guesses {
        codes.push(guess.await.unwrap());
    }
    codes.sort();
    assert_eq!(
        codes,
        [vec!["second_factor_invalid"; 5], vec!["unauthenticated"; 3]].concat()
    );
    let right = command(
        &harness,
        Some(&challenge),
        "auth.verifySecondFactor",
        json!({ "kind": "totp", "code": next_code(&secret) }),
    )
    .await;
    assert_eq!(right.code(), "unauthenticated");
}

/// AX-S019: a recovery code signs in once, typed in any case and with spaces.
#[tokio::test]
async fn a_recovery_code_works_once() {
    let harness = Harness::new().await;
    let (_, codes) = enroll(&harness, &harness.session("p_2fa_enrolled").await).await;
    let typed = codes[0].replace('-', " ").to_lowercase();

    let challenge = set_cookie(
        &login(&harness, "p_2fa_enrolled", PASSWORD).await,
        "holocron_challenge",
    )
    .unwrap();
    let first = command(
        &harness,
        Some(&challenge),
        "auth.verifySecondFactor",
        json!({ "kind": "recovery_code", "code": typed }),
    )
    .await;
    assert_eq!(first.json()["status"], "signed_in", "{}", first.json());
    let session = set_cookie(&first, "holocron_session").unwrap();
    let status = harness
        .query(Some(&session), "twoFactor.status", "")
        .await
        .json();
    assert_eq!(status["recovery_codes_left"], 9);

    let challenge = set_cookie(
        &login(&harness, "p_2fa_enrolled", PASSWORD).await,
        "holocron_challenge",
    )
    .unwrap();
    let second = command(
        &harness,
        Some(&challenge),
        "auth.verifySecondFactor",
        json!({ "kind": "recovery_code", "code": codes[0] }),
    )
    .await;
    assert_eq!(second.code(), "second_factor_invalid");
}

/// AX-S020 and decision 0007: a required but unenrolled account gets an enrollment-only
/// session, which confirming enrollment turns into a full one.
#[tokio::test]
async fn enrollment_only_sessions_become_full_after_enrollment() {
    let harness = Harness::new().await;
    require_two_factor_in_ws_a(&harness).await;

    let answer = login(&harness, "p_2fa_unenrolled", PASSWORD).await;
    assert_eq!(
        answer.json()["status"],
        "enrollment_required",
        "{}",
        answer.json()
    );
    let session = set_cookie(&answer, "holocron_session").unwrap();
    let refused = site_get(&harness, &session).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.code(), "second_factor_enrollment_required");
    assert_eq!(
        harness
            .query(Some(&session), "account.get", "")
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        harness
            .query(Some(&session), "auth.session", "")
            .await
            .json()["enrollment_only"],
        true
    );

    enroll(&harness, &session).await;
    assert_eq!(site_get(&harness, &session).await.status, StatusCode::OK);
    assert_eq!(
        harness
            .query(Some(&session), "auth.session", "")
            .await
            .json()["enrollment_only"],
        false
    );
}

/// AX-S027, AX-T075, AX-T077: a requirement switched on applies at the next request without a
/// grace period; the account's API keys keep working. Only the right people may switch it.
#[tokio::test]
async fn a_new_requirement_drops_existing_sessions_to_enrollment_only() {
    let harness = Harness::new().await;
    let session = harness.session("p_2fa_unenrolled").await;
    let key = harness
        .api_key("p_2fa_unenrolled", &["sites.get"], None)
        .await;
    assert_eq!(site_get(&harness, &session).await.status, StatusCode::OK);

    let owner = harness.session("p_owner").await;
    let member = harness.session("p_member").await;
    let installation =
        json!({ "target": { "scope": "installation" }, "two_factor_required": true });
    let refused = command(
        &harness,
        Some(&owner),
        "security.updatePolicy",
        installation,
    )
    .await;
    assert_eq!(refused.code(), "forbidden");
    let team = json!({ "target": { "scope": "team", "workspace": harness.workspaces["ws_a"] }, "two_factor_required": true });
    let refused = command(&harness, Some(&member), "security.updatePolicy", team).await;
    assert_eq!(refused.code(), "forbidden");

    require_two_factor_in_ws_a(&harness).await;
    assert_eq!(
        site_get(&harness, &session).await.code(),
        "second_factor_enrollment_required"
    );
    let site = &harness.sites["site_a1"];
    let by_key = harness
        .query_with(&key, "sites.get", &format!("site={site}"))
        .await;
    assert_eq!(by_key.status, StatusCode::OK);
    // The owner is unenrolled too now; an install admin outside the team still reads it.
    let admin = harness.session("p_install_admin").await;
    let policy = harness
        .query(Some(&admin), "security.policy", "")
        .await
        .json();
    assert_eq!(policy["teams"][0]["two_factor_required"], true);
}

/// AX-S021: a password change needs a fresh login and ends every other session.
#[tokio::test]
async fn a_password_change_ends_other_sessions() {
    let harness = Harness::new().await;
    let other = harness.session("p_member").await;
    let current = harness.session_reauthenticated("p_member", None).await;
    let change = json!({ "new_password": NEW_PASSWORD });

    let refused = command(
        &harness,
        Some(&current),
        "account.changePassword",
        change.clone(),
    )
    .await;
    assert_eq!(refused.code(), "reauthentication_required");
    let wrong = command(
        &harness,
        Some(&current),
        "auth.reauthenticate",
        json!({ "password": "not the password" }),
    )
    .await;
    assert_eq!(wrong.code(), "invalid_credentials");
    ok(
        &harness,
        &current,
        "auth.reauthenticate",
        json!({ "password": PASSWORD }),
    )
    .await;
    ok(&harness, &current, "account.changePassword", change).await;

    assert_eq!(
        harness.query(Some(&other), "account.get", "").await.code(),
        "unauthenticated"
    );
    assert_eq!(
        harness
            .query(Some(&current), "account.get", "")
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        login(&harness, "p_member", PASSWORD).await.code(),
        "invalid_credentials"
    );
    assert_eq!(
        login(&harness, "p_member", NEW_PASSWORD).await.json()["status"],
        "signed_in"
    );
}

/// AX-S022: `auth.revokeSessions` ends every other session and keeps the current one.
#[tokio::test]
async fn revoke_sessions_keeps_only_the_current_one() {
    let harness = Harness::new().await;
    let current = harness.session("p_member").await;
    let other = harness.session("p_member").await;
    let answer = ok(&harness, &current, "auth.revokeSessions", json!({})).await;
    assert_eq!(answer["revoked"], 1);
    assert_eq!(
        harness.query(Some(&other), "account.get", "").await.code(),
        "unauthenticated"
    );
    assert_eq!(
        harness
            .query(Some(&current), "account.get", "")
            .await
            .status,
        StatusCode::OK
    );
}

/// Decision 0012: a session unused for 7 days ends.
#[tokio::test]
async fn an_idle_session_ends() {
    let harness = Harness::new().await;
    let session = harness.session("p_member").await;
    sqlx::query("UPDATE sessions SET last_seen_at = ?")
        .bind(now_ms() - 8 * 86_400_000)
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    assert_eq!(
        harness
            .query(Some(&session), "account.get", "")
            .await
            .code(),
        "unauthenticated"
    );
}

/// AX-T133, AX-T135: reauthentication checks the password, and the second factor when enrolled.
#[tokio::test]
async fn reauthentication_needs_the_second_factor_when_enrolled() {
    let harness = Harness::new().await;
    let session = harness.session("p_2fa_enrolled").await;
    let (_, codes) = enroll(&harness, &session).await;
    harness.set_reauthenticated_at(&session, None).await;

    let password_only = command(
        &harness,
        Some(&session),
        "auth.reauthenticate",
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(password_only.code(), "second_factor_required");
    let wrong = command(
        &harness,
        Some(&session),
        "auth.reauthenticate",
        json!({ "password": PASSWORD, "second_factor": { "kind": "totp", "code": "000000" } }),
    )
    .await;
    assert_eq!(wrong.code(), "second_factor_invalid");
    let refused = command(
        &harness,
        Some(&session),
        "twoFactor.regenerateRecoveryCodes",
        json!({}),
    )
    .await;
    assert_eq!(refused.code(), "reauthentication_required");

    let passed = ok(&harness, &session, "auth.reauthenticate", json!({ "password": PASSWORD, "second_factor": { "kind": "recovery_code", "code": codes[1] } })).await;
    assert!(passed["reauthenticated_until"].as_i64().unwrap() > now_ms());
    let fresh = ok(
        &harness,
        &session,
        "twoFactor.regenerateRecoveryCodes",
        json!({}),
    )
    .await;
    assert_eq!(fresh["secret"].as_array().unwrap().len(), 10);
}

/// AX-S006: a key secret is disclosed once; a replay returns the same key without it, and
/// stored answers never hold it.
#[tokio::test]
async fn key_secrets_are_withheld_on_replay() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let body = json!({
        "idempotency_key": "K6",
        "name": "ci",
        "operations": ["sites.get"],
        "sites": { "kind": "only", "sites": [harness.sites["site_a1"]] },
    });

    let first = harness
        .command(Some(&member), "apiKeys.create", body.clone())
        .await
        .json();
    let secret = first["secret"].as_str().unwrap().to_owned();
    let replay = harness
        .command(Some(&member), "apiKeys.create", body)
        .await
        .json();
    assert_eq!(replay["key"]["id"], first["key"]["id"]);
    assert_eq!(replay["secret"], Value::Null);
    assert_eq!(replay["secret_withheld"], true);

    let keys = harness
        .query(Some(&member), "apiKeys.list", "")
        .await
        .json();
    assert_eq!(keys["keys"].as_array().unwrap().len(), 1);
    assert!(!keys.to_string().contains(&secret));
    let stored: Vec<String> = sqlx::query_scalar("SELECT response FROM idempotency_keys")
        .fetch_all(&harness.state.db.reader)
        .await
        .unwrap();
    assert!(stored.iter().all(|response| !response.contains(&secret)));

    let site = &harness.sites["site_a1"];
    let used = harness
        .query_with(
            &format!("Bearer {secret}"),
            "sites.get",
            &format!("site={site}"),
        )
        .await;
    assert_eq!(used.status, StatusCode::OK);
}

/// AX-T057 to AX-T060, AX-T130: a key never holds more than its owner, nor any account or
/// reauthentication-gated operation, and a site the owner cannot see is `not_found`.
#[tokio::test]
async fn key_grants_never_exceed_the_owner() {
    let harness = Harness::new().await;
    let create = |alias: &'static str, operations: Value, sites: Value| {
        let harness = &harness;
        async move {
            let session = harness.session(alias).await;
            let sites = json!({ "kind": "only", "sites": sites.as_array().unwrap().iter().map(|s| harness.sites[s.as_str().unwrap()].to_string()).collect::<Vec<_>>() });
            command(
                harness,
                Some(&session),
                "apiKeys.create",
                json!({ "name": "k", "operations": operations, "sites": sites }),
            )
            .await
        }
    };

    assert_eq!(
        create("p_viewer", json!(["analytics.export"]), json!(["site_a1"]))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        create("p_viewer", json!(["sites.update"]), json!(["site_a1"]))
            .await
            .code(),
        "grant_exceeds_principal"
    );
    assert_eq!(
        create("p_member", json!(["auth.login"]), json!([]))
            .await
            .code(),
        "grant_exceeds_principal"
    );
    assert_eq!(
        create("p_owner", json!(["sites.delete"]), json!(["site_a1"]))
            .await
            .code(),
        "grant_exceeds_principal"
    );
    assert_eq!(
        create("p_member", json!(["analytics.explore"]), json!(["site_b1"]))
            .await
            .code(),
        "not_found"
    );
    assert_eq!(
        create("p_member", json!(["no.such"]), json!([]))
            .await
            .code(),
        "invalid_arguments"
    );
}

/// AX-S009, AX-S014: a revoked key answers exactly like an unknown one, at once.
#[tokio::test]
async fn a_revoked_key_loses_access_at_once() {
    let harness = Harness::new().await;
    let member = harness.session("p_member").await;
    let created = ok(
        &harness,
        &member,
        "apiKeys.create",
        json!({ "name": "ci", "operations": ["sites.get"], "sites": { "kind": "all" } }),
    )
    .await;
    let key = format!("Bearer {}", created["secret"].as_str().unwrap());
    let query = format!("site={}", harness.sites["site_a1"]);
    let read = || harness.query_with(&key, "sites.get", &query);
    assert_eq!(read().await.status, StatusCode::OK);

    let revoked = ok(
        &harness,
        &member,
        "apiKeys.revoke",
        json!({ "key": created["key"]["id"] }),
    )
    .await;
    assert!(revoked["key"]["revoked_at"].is_i64());
    let after = read().await;
    let unknown = harness
        .query_with("Bearer unknown", "sites.get", &query)
        .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(after.bytes, unknown.bytes);
}

/// AX-S025: a missing fresh login is refused before the idempotency check and stores nothing;
/// a stored answer is not replayed once the session's check has expired.
#[tokio::test]
async fn reauthentication_runs_before_replay() {
    let harness = Harness::new().await;
    let session = harness.session_reauthenticated("p_admin", None).await;
    let body = json!({ "idempotency_key": "K8", "name": "ci", "operations": ["sites.get"], "sites": { "kind": "all" } });

    let refused = harness
        .command(Some(&session), "apiKeys.create", body.clone())
        .await;
    assert_eq!(refused.code(), "reauthentication_required");
    ok(
        &harness,
        &session,
        "auth.reauthenticate",
        json!({ "password": PASSWORD }),
    )
    .await;
    let created = harness
        .command(Some(&session), "apiKeys.create", body.clone())
        .await;
    assert_eq!(
        created.json()["secret_withheld"],
        false,
        "{}",
        created.json()
    );

    let eleven_minutes_ago = now_ms() - REAUTHENTICATION_WINDOW_MS - 60_000;
    harness
        .set_reauthenticated_at(&session, Some(eleven_minutes_ago))
        .await;
    let replay = harness
        .command(Some(&session), "apiKeys.create", body)
        .await;
    assert_eq!(replay.code(), "reauthentication_required");
}

/// AX-S026, AX-T142 to AX-T145: erasure refuses owners and the last install admin, then ends
/// sessions and keys, deletes own explorations and leaves a tombstone.
#[tokio::test]
async fn users_delete_erases_the_account() {
    let harness = Harness::new().await;
    let admin = harness.session("p_install_admin").await;
    let member = harness.session("p_member").await;
    let key = harness.api_key("p_member", &["sites.get"], None).await;
    let user = harness.users["p_member"].clone();
    sqlx::query("INSERT INTO saved_explorations (id, site_id, owner_id, name, state, created_at, updated_at) VALUES ('own', ?, ?, 'mine', '{}', 0, 0)")
        .bind(harness.sites["site_a1"].as_str())
        .bind(user.as_str())
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    let delete = |alias: &str| json!({ "user": harness.users[alias] });

    let owner = harness.session("p_owner").await;
    assert_eq!(
        command(&harness, Some(&owner), "users.delete", delete("p_member"))
            .await
            .code(),
        "forbidden"
    );
    let owns = command(
        &harness,
        Some(&admin),
        "users.delete",
        delete("p_other_owner"),
    )
    .await;
    assert_eq!(owns.code(), "user_owns_resources");
    assert_eq!(
        owns.json()["owned"][0]["id"],
        json!(harness.workspaces["ws_b"])
    );
    ok(
        &harness,
        &admin,
        "users.delete",
        delete("p_install_admin_member"),
    )
    .await;
    assert_eq!(
        command(
            &harness,
            Some(&admin),
            "users.delete",
            delete("p_install_admin")
        )
        .await
        .code(),
        "last_install_admin"
    );

    let deleted = ok(&harness, &admin, "users.delete", delete("p_member")).await;
    assert_eq!(deleted["user"], json!(user));
    assert_eq!(
        harness.query(Some(&member), "account.get", "").await.code(),
        "unauthenticated"
    );
    let site = &harness.sites["site_a1"];
    assert_eq!(
        harness
            .query_with(&key, "sites.get", &format!("site={site}"))
            .await
            .code(),
        "unauthenticated"
    );
    let remaining: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM saved_explorations WHERE id = 'own'),
                (SELECT COUNT(*) FROM api_keys WHERE owner_id = ?1),
                (SELECT COUNT(*) FROM user_tombstones WHERE user_id = ?1)",
    )
    .bind(user.as_str())
    .fetch_one(&harness.state.db.reader)
    .await
    .unwrap();
    assert_eq!(remaining, (0, 0, 1));
}

/// `users.create` discloses a generated password once; it signs in and replays withhold it.
#[tokio::test]
async fn users_create_discloses_a_generated_password_once() {
    let harness = Harness::new().await;
    let admin = harness.session("p_install_admin").await;
    let body = json!({ "idempotency_key": "u1", "email": "New@Example.test", "name": "New", "install_role": "user" });
    let created = harness
        .command(Some(&admin), "users.create", body.clone())
        .await
        .json();
    let password = created["secret"].as_str().unwrap().to_owned();
    assert_eq!(created["user"]["email"], "new@example.test");
    assert_eq!(
        harness
            .command(Some(&admin), "users.create", body)
            .await
            .json()["secret_withheld"],
        true
    );

    let signed_in = command(
        &harness,
        None,
        "auth.login",
        json!({ "email": "new@example.test", "password": password }),
    )
    .await;
    assert_eq!(signed_in.json()["status"], "signed_in");
    let taken = command(
        &harness,
        Some(&admin),
        "users.create",
        json!({ "email": "new@example.test", "name": "Again", "install_role": "user" }),
    )
    .await;
    assert_eq!(taken.code(), "invalid_arguments");
}

/// AX-S023, decision 0012: init writes the 2FA key file readable by its owner only, and no
/// known default account exists.
#[tokio::test]
async fn init_creates_the_secrets_key_and_no_default_login() {
    let dir = tempfile::TempDir::new().unwrap();
    let data = dir.path().join("data");
    init::init(InitArgs {
        data_dir: data.clone(),
        admin_email: "admin@example.test".into(),
        admin_password: None,
    })
    .await
    .unwrap();
    let key = std::fs::metadata(data.join(SECRETS_KEY_FILE)).unwrap();
    assert_eq!(key.len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(key.permissions().mode() & 0o777, 0o600);
    }

    let harness = Harness::new().await;
    let umami = command(
        &harness,
        None,
        "auth.login",
        json!({ "email": "admin", "password": "umami" }),
    )
    .await;
    assert_eq!(umami.code(), "invalid_credentials");
}
