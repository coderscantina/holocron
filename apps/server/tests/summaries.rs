//! Dashboard v2 v02: the site summaries step with a pinned clock. When it runs and when it
//! waits, that its row is what `analytics.explore` shows for the same state, staleness,
//! erasure, access and the stream event; and its findings: every rule family on a
//! hand-computed fixture with near misses, their ranking, that each stored state shows the
//! stated values, and that a run replaces the set while hourly rules keep theirs.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, header};
use holocron_protocol::ids::SiteId;
use holocron_server::clock::Clock;
use holocron_server::config::{CollectConfig, parse_net};
use holocron_server::maintenance::jobs::{self, Job};
use holocron_server::summaries::{self, Round, Schedule};
use holocron_storage::{Budgets, Lane};
use serde_json::{Value, json};
use tokio::sync::watch;

use common::{HOST, Harness, frames, test_budgets};

/// Tuesday 2026-10-13, 15:20 in Vienna (CEST).
const TUESDAY_1520: i64 = 1_791_897_600_000;
/// 2023-11-14T22:13:20Z, for tests that need no history.
const LONG_AGO: i64 = 1_700_000_000_000;
const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const WEEK_MS: i64 = 7 * 24 * HOUR_MS;
const DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
const PHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
const DAY_MS: i64 = 24 * HOUR_MS;

async fn harness(budgets: Budgets, now_ms: i64) -> Harness {
    let collect = CollectConfig {
        trusted_proxies: vec![parse_net("127.0.0.1").unwrap()],
        ..CollectConfig::default()
    };
    Harness::with(collect, budgets, Clock::frozen(now_ms)).await
}

/// One pageview at `at_ms` from a visitor at `ip`.
async fn pageview(harness: &Harness, site: &str, ip: &str, at_ms: i64) {
    send(
        harness,
        site,
        ip,
        DESKTOP,
        at_ms,
        "event",
        json!({ "url": "/" }),
    )
    .await;
}

/// One record of `kind` at `at_ms` from a visitor at `ip` with `agent`; `extra` adds to or
/// replaces payload fields.
async fn send(
    harness: &Harness,
    site: &str,
    ip: &str,
    agent: &str,
    at_ms: i64,
    kind: &str,
    extra: Value,
) {
    let mut payload = json!({
        "website": site, "hostname": "example.test", "url": "/", "language": "de-AT",
        "referrer": "", "timestamp": at_ms / 1000,
    });
    payload
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let request = Request::post("/api/send")
        .header(header::HOST, HOST)
        .header(header::USER_AGENT, agent)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", ip)
        .extension(axum::extract::ConnectInfo(
            "127.0.0.1:9000".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .body(Body::from(
            json!({ "type": kind, "payload": payload }).to_string(),
        ))
        .unwrap();
    assert_eq!(harness.send(request).await.status, 200);
}

async fn settle(harness: &Harness) {
    let storage = harness.state.collector.storage();
    storage.drain().await;
    holocron_analytics::run_until_clean(storage, holocron_analytics::Limits::default())
        .await
        .unwrap();
}

/// A Vienna site with four earlier Tuesdays of two pageviews at 10:00 and one at 16:00, and
/// three pageviews today at 09:00, 11:00 and 15:00.
async fn traffic(harness: &Harness) -> String {
    let site = harness.site("Europe/Vienna").await;
    let mut ip = 0;
    let mut send = |at_ms| {
        ip += 1;
        (format!("203.0.113.{ip}"), at_ms)
    };
    let mut hits = Vec::new();
    for week in 1..=4 {
        let tuesday = TUESDAY_1520 - week * WEEK_MS;
        let ten = tuesday - 5 * HOUR_MS - 20 * MINUTE_MS;
        hits.extend([send(ten), send(ten), send(tuesday + 40 * MINUTE_MS)]);
    }
    for hours in [6, 4, 0] {
        hits.push(send(TUESDAY_1520 - hours * HOUR_MS - 20 * MINUTE_MS));
    }
    for (ip, at_ms) in hits {
        pageview(harness, &site, &ip, at_ms).await;
    }
    settle(harness).await;
    site
}

async fn round(harness: &Harness, schedule: &mut Schedule) -> Round {
    let (_stop, stopped) = watch::channel(false);
    summaries::round(&harness.state, schedule, &stopped)
        .await
        .unwrap()
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn ok(answer: &common::Answer) -> Value {
    assert_eq!(
        answer.status,
        200,
        "{}",
        String::from_utf8_lossy(&answer.bytes)
    );
    answer.json()
}

async fn summaries_of(harness: &Harness, cookie: &str, sites: &[&str]) -> Vec<Value> {
    let query = sites
        .iter()
        .map(|site| format!("sites={site}"))
        .collect::<Vec<_>>()
        .join("&");
    let answer = harness
        .query(Some(cookie), "summaries.forSites", &query)
        .await;
    ok(&answer)["summaries"].as_array().unwrap().clone()
}

fn written(round: &Round, site: &str) -> bool {
    round.written.iter().any(|s| s.as_str() == site)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_row_is_what_explore_shows_and_waits_for_the_watermark_and_cadence() {
    let h = harness(test_budgets(), TUESDAY_1520).await;
    let site = traffic(&h).await;
    let owner = h.session("p_owner").await;
    let mut schedule = Schedule::default();

    assert!(written(&round(&h, &mut schedule).await, &site));
    let row = summaries_of(&h, &owner, &[&site]).await.remove(0);

    // The same state at the same snapshot through `analytics.explore`.
    let state = summaries::state(&SiteId::new(site.clone()), "Europe/Vienna");
    let result = ok(&h
        .query(
            Some(&owner),
            "analytics.explore",
            &format!("state={}", encode(&serde_json::to_string(&state).unwrap())),
        )
        .await);
    let main = &result["main"];
    for (i, measure) in ["pageviews", "visitor_days"].into_iter().enumerate() {
        let pace = &row[measure];
        assert_eq!(pace["today"], main["totals"][i]["v"], "{measure}");
        assert_eq!(
            pace["typical"], main["compare"][i]["previous"]["v"],
            "{measure}"
        );
        assert_eq!(
            pace["relative_change"],
            main["compare"][i]["relative_change"]
        );
        assert_eq!(pace["projection"], main["projection"][i], "{measure}");
    }
    let points = main["series"]["points"].as_array().unwrap();
    let today: Vec<Value> = points.iter().map(|p| p["values"][0]["v"].clone()).collect();
    let typical: Vec<Value> = points
        .iter()
        .map(|p| p["compare"][0]["previous"]["v"].clone())
        .collect();
    assert_eq!(row["hourly_today"], json!(today));
    assert_eq!(row["hourly_typical"], json!(typical));
    assert_eq!(
        (&row["watermark_batch"], &row["watermark_sequence"]),
        (
            &result["freshness"]["watermark_batch"],
            &result["freshness"]["watermark_sequence"]
        )
    );
    // And by hand: 3 pageviews so far against the median 2 of four Tuesdays by 15:20.
    assert_eq!(row["pageviews"]["today"], 3);
    assert_eq!(row["pageviews"]["typical"], 2);
    assert_eq!(row["pageviews"]["relative_change"], 0.5);
    assert_eq!(row["pageviews"]["projection"]["basis_days"], 4);
    assert_eq!(row["hourly_today"].as_array().unwrap().len(), 24);
    assert_eq!(
        (
            &row["hourly_today"][9],
            &row["hourly_today"][15],
            &row["hourly_typical"][10]
        ),
        (&json!(1), &json!(1), &json!(2))
    );
    assert_eq!(
        (
            &row["insufficient_history"],
            &row["stale"],
            &row["finding_count"]
        ),
        (&json!(false), &json!(false), &json!(0))
    );
    assert_eq!(row["computed_at"], TUESDAY_1520);

    // Nothing moved: not due.
    assert!(!written(&round(&h, &mut schedule).await, &site));
    // The watermark moved, but the cadence holds it until 10 minutes passed.
    pageview(&h, &site, "198.51.100.1", TUESDAY_1520 - MINUTE_MS).await;
    settle(&h).await;
    let clock = h.state.collector.clock();
    assert!(!written(&round(&h, &mut schedule).await, &site));
    clock.set(TUESDAY_1520 + 10 * MINUTE_MS - 1);
    assert!(!written(&round(&h, &mut schedule).await, &site));
    clock.set(TUESDAY_1520 + 10 * MINUTE_MS);
    assert!(written(&round(&h, &mut schedule).await, &site));
    let row = summaries_of(&h, &owner, &[&site]).await.remove(0);
    assert_eq!(row["pageviews"]["today"], 4);
    let computed_at = row["computed_at"].as_i64().unwrap();

    // Stale after 30 minutes, or under another metric version.
    clock.set(computed_at + 30 * MINUTE_MS);
    assert_eq!(summaries_of(&h, &owner, &[&site]).await[0]["stale"], false);
    clock.set(computed_at + 30 * MINUTE_MS + 1);
    assert_eq!(summaries_of(&h, &owner, &[&site]).await[0]["stale"], true);
    clock.set(computed_at + 31 * MINUTE_MS);
    sqlx::query("UPDATE site_summaries SET computed_at = ?, metric_version = 0 WHERE site_id = ?")
        .bind(computed_at + 31 * MINUTE_MS)
        .bind(&site)
        .execute(&h.state.db.writer)
        .await
        .unwrap();
    assert_eq!(summaries_of(&h, &owner, &[&site]).await[0]["stale"], true);

    // Operators see the step's counters.
    h.state.collector.save_counters().await.unwrap();
    let admin = h.session("p_install_admin").await;
    let status = ok(&h.query(Some(&admin), "operator.status", "").await);
    let runs = status["collection"]["counters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["counter"] == "summary_runs")
        .map(|c| c["count"].as_i64().unwrap());
    assert!(runs >= Some(2), "{runs:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_round_waits_while_a_report_waits_or_transfer_lags() {
    let h = harness(test_budgets(), LONG_AGO).await;
    let site = h.site("UTC").await;
    let storage = h.state.collector.storage();
    let mut schedule = Schedule::default();

    // Both report workers busy: a summary would wait, so none starts.
    let mut held = Vec::new();
    let mut readers = Vec::new();
    for _ in 0..2 {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        held.push(release);
        let state = h.state.clone();
        readers.push(tokio::spawn(async move {
            state
                .collector
                .storage()
                .read(Lane::Report, move |_| {
                    let _ = wait.recv();
                    Ok(())
                })
                .await
                .unwrap();
        }));
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while storage.reports_idle() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let busy = round(&h, &mut schedule).await;
    assert!(busy.written.is_empty() && busy.skipped > 0, "{busy:?}");
    drop(held);
    for reader in readers {
        reader.await.unwrap();
    }

    // Transfer held behind a write while a record waits longer than 10 s. The inbox records
    // wall time, so this waits for real.
    let (started, running) = std::sync::mpsc::channel::<()>();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let state = h.state.clone();
    let writer = tokio::spawn(async move {
        state
            .collector
            .storage()
            .write(move |_| {
                let _ = started.send(());
                let _ = wait.recv();
                Ok(())
            })
            .await
            .unwrap();
    });
    tokio::task::spawn_blocking(move || running.recv().unwrap())
        .await
        .unwrap();
    pageview(&h, &site, "203.0.113.9", LONG_AGO).await;
    tokio::time::sleep(Duration::from_millis(10_200)).await;
    let lagging = round(&h, &mut schedule).await;
    assert!(
        lagging.written.is_empty() && lagging.skipped > 0,
        "{lagging:?}"
    );
    drop(release);
    writer.await.unwrap();
    settle(&h).await;

    let idle = round(&h, &mut schedule).await;
    assert!(written(&idle, &site), "{idle:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timed_out_site_is_skipped_and_retried_next_round() {
    // Every report read hits the timeout before a worker answers.
    let budgets = Budgets {
        query_timeout: Duration::ZERO,
        ..test_budgets()
    };
    let h = harness(budgets, LONG_AGO).await;
    let site = h.site("UTC").await;
    // Only this site: a cancelled read holds its queue slot until a worker skips it, so a
    // round over many timing-out sites would stop early on a busy lane.
    sqlx::query("DELETE FROM sites WHERE id <> ?")
        .bind(&site)
        .execute(&h.state.db.writer)
        .await
        .unwrap();
    let storage = h.state.collector.storage();
    let mut schedule = Schedule::default();

    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !storage.reports_idle() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let round = round(&h, &mut schedule).await;
        assert!(
            round.timed_out.iter().any(|s| s.as_str() == site) && round.written.is_empty(),
            "{round:?}"
        );
    }
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM site_summaries")
        .fetch_one(&h.state.db.reader)
        .await
        .unwrap();
    assert_eq!(stored, 0);
    h.state.collector.save_counters().await.unwrap();
    let timeouts: i64 = sqlx::query_scalar(
        "SELECT count FROM collection_counter_counts WHERE counter = 'summary_timeouts'",
    )
    .fetch_one(&h.state.db.reader)
    .await
    .unwrap();
    assert!(timeouts >= 2, "{timeouts}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_and_erasure_remove_the_summary_and_findings() {
    let h = harness(test_budgets(), LONG_AGO).await;
    let site = h.site("UTC").await;
    let uuid = uuid::Uuid::parse_str(&site).unwrap();
    let mut schedule = Schedule::default();
    let rows = || {
        let (pool, site) = (h.state.db.reader.clone(), site.clone());
        async move {
            sqlx::query_as::<_, (i64, i64)>(
                "SELECT (SELECT count(*) FROM site_summaries WHERE site_id = ?1),
                        (SELECT count(*) FROM site_findings WHERE site_id = ?1)",
            )
            .bind(site)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert!(written(&round(&h, &mut schedule).await, &site));
    sqlx::query(
        "INSERT INTO site_findings VALUES (?, 1, 'dimension_mover', 1, 'mover', '{}', '{}', 0, 0.5, 0)",
    )
    .bind(&site)
    .execute(&h.state.db.writer)
    .await
    .unwrap();

    // A queued reset keeps the step away from the site until its job is done.
    let now = h.state.collector.clock().now_ms();
    let through_seq = h.state.collector.storage().last_sequence().await.unwrap();
    let reset = jobs::enqueue(
        &h.state.db.writer,
        &Job::ResetSite {
            site: uuid,
            through_seq,
        },
        now,
    )
    .await
    .unwrap();
    assert!(!written(&round(&h, &mut schedule).await, &site));
    jobs::run_until(&h.state, &reset).await.unwrap();
    assert_eq!(rows().await, (0, 0));
    // A step that started when the job was queued may hold erased data; one after it may not.
    h.state.collector.clock().set(now + 1);
    // The reset site is summarized again from what is left.
    assert!(written(&round(&h, &mut schedule).await, &site));

    sqlx::query("DELETE FROM sites WHERE id = ?")
        .bind(&site)
        .execute(&h.state.db.writer)
        .await
        .unwrap();
    let erase = jobs::enqueue(&h.state.db.writer, &Job::EraseSite { site: uuid }, now)
        .await
        .unwrap();
    jobs::run_until(&h.state, &erase).await.unwrap();
    assert_eq!(rows().await, (0, 0));
    assert!(!written(&round(&h, &mut schedule).await, &site));
}

#[tokio::test(flavor = "multi_thread")]
async fn only_readers_get_a_summary_or_findings_and_their_stream_hears_of_it() {
    let h = harness(test_budgets(), LONG_AGO).await;
    let site = h.site("UTC").await;
    let member = h.session("p_member").await;
    let stream = h.subscribe(("cookie", &member), &site).await;
    let mut schedule = Schedule::default();
    assert!(written(&round(&h, &mut schedule).await, &site));

    let other = h.sites["site_b1"].as_str().to_owned();
    let guess = h.sites["site_guess"].as_str().to_owned();
    let mine = summaries_of(&h, &member, &[&guess, &site, &other, &site]).await;
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["site"], site.as_str());
    let outsider = h.session("p_other_owner").await;
    let hidden = h
        .query(
            Some(&outsider),
            "summaries.forSites",
            &format!("sites={site}"),
        )
        .await;
    let missing = h
        .query(
            Some(&outsider),
            "summaries.forSites",
            &format!("sites={guess}"),
        )
        .await;
    assert_eq!(ok(&hidden)["summaries"], json!([]));
    assert_eq!(hidden.bytes, missing.bytes);

    let frames = frames(stream.into_body(), 2).await;
    assert_eq!(frames[0]["kind"], "subscribed");
    assert_eq!(frames[1]["kind"], "events");
    assert_eq!(frames[1]["events"][0]["kind"], "summary");

    // Findings: readers get the site's list, anyone else the answer of an unknown site.
    let mine = findings_of(&h, &member, &site).await;
    assert_eq!(
        (&mine["findings"], &mine["stale"]),
        (&json!([]), &json!(false))
    );
    let findings = |cookie: &str, site: &str| {
        let query = format!("site={site}&limit=3");
        let cookie = cookie.to_owned();
        let h = &h;
        async move { h.query(Some(&cookie), "summaries.findings", &query).await }
    };
    let hidden = findings(&outsider, &site).await;
    let missing = findings(&outsider, &guess).await;
    assert_eq!(hidden.code(), "not_found");
    assert_eq!(hidden.bytes, missing.bytes);
    for limit in [0, 21] {
        let query = format!("site={site}&limit={limit}");
        let refused = h.query(Some(&member), "summaries.findings", &query).await;
        assert_eq!(refused.code(), "invalid_arguments", "{limit}");
    }

    let too_many = (0..101)
        .map(|_| format!("sites={site}"))
        .collect::<Vec<_>>();
    let refused = h
        .query(Some(&member), "summaries.forSites", &too_many.join("&"))
        .await;
    assert_eq!(refused.code(), "invalid_arguments");
}

async fn command(harness: &Harness, cookie: &str, name: &str, mut body: Value) -> Value {
    body["idempotency_key"] = json!(uuid::Uuid::new_v4().to_string());
    ok(&harness.command(Some(cookie), name, body).await)
}

async fn findings_of(harness: &Harness, cookie: &str, site: &str) -> Value {
    ok(&harness
        .query(
            Some(cookie),
            "summaries.findings",
            &format!("site={site}&limit=20"),
        )
        .await)
}

fn predicate(id: &str, field: &str, value: &str) -> Value {
    json!({ "kind": "group", "op": "and", "items": [{
        "kind": "cond", "id": id, "field": field, "cmp": "eq",
        "value": { "t": "string", "v": value }
    }]})
}

fn funnel(site: &str, entry: &str) -> Value {
    json!({
        "v": 1, "site": site, "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 7 }, "timezone": "Europe/Vienna",
        "granularity": "day", "compare": "none", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "funnel",
        "report": { "kind": "funnel", "v": 1, "window_seconds": 1800, "scope": "daily", "steps": [
            predicate("s1", "path", entry), predicate("s2", "event_name", "buy"),
        ]}
    })
}

/// Visitors of one day, all at `ten` (10:00 local): `sale` on /sale of whom `buy` buy five
/// minutes later, `near` on /near, and 20 on / of whom `signups` sign up and `newsletters`
/// subscribe. Returns the first visitor on /.
#[allow(clippy::too_many_arguments)]
async fn day_of_traffic(
    harness: &Harness,
    site: &str,
    ip: &mut impl FnMut() -> String,
    ten: i64,
    (sale, buy): (usize, usize),
    near: usize,
    (signups, newsletters): (usize, usize),
) -> String {
    for i in 0..sale {
        let visitor = ip();
        send(
            harness,
            site,
            &visitor,
            DESKTOP,
            ten,
            "event",
            json!({ "url": "/sale" }),
        )
        .await;
        if i < buy {
            let at = ten + 5 * MINUTE_MS;
            let buy = json!({ "url": "/sale", "name": "buy" });
            send(harness, site, &visitor, DESKTOP, at, "event", buy).await;
        }
    }
    for _ in 0..near {
        send(
            harness,
            site,
            &ip(),
            DESKTOP,
            ten,
            "event",
            json!({ "url": "/near" }),
        )
        .await;
    }
    let mut first = None;
    for i in 0..20 {
        let visitor = ip();
        send(harness, site, &visitor, DESKTOP, ten, "event", json!({})).await;
        for (n, name, after) in [(signups, "signup", 1), (newsletters, "newsletter", 2)] {
            if i < n {
                let at = ten + after * MINUTE_MS;
                send(
                    harness,
                    site,
                    &visitor,
                    DESKTOP,
                    at,
                    "event",
                    json!({ "name": name }),
                )
                .await;
            }
        }
        first.get_or_insert(visitor);
    }
    first.unwrap()
}

/// The hand-computed fixture of every rule family, in Vienna, today Tuesday 2026-10-13 at
/// 15:20, four earlier Tuesdays of history:
///
/// - Visitor-days by 15:20: typical 45 (15 /sale, 10 /near, 20 /), today 109 (60, 29, 20): the
///   site changed by 64. /sale moved by 45 (4 times, share 0.7031); /near by 19, a near miss.
///   Every visitor has the same referrer, browser, os and device, so each of those moved with
///   the site (share 1).
/// - Goals: `signup` 10 typical, 15 today (+5, +50 %); `newsletter` 14, a near miss.
/// - Funnel "Sale to buy": today 60 enter, 30 drop (50 %); pooled baseline 60 enter, 24 drop
///   (40 %), 10 points. "Near to buy" has 29 entering today, a near miss.
/// - Performance: on four days before yesterday one phone and one desktop observation each,
///   phone LCP and FCP 2000, 2400, 1800, 2200. Today 100 phone observations at LCP 2500
///   (slowest was 2400) and FCP 2400 (equal, a near miss), and 99 desktop observations, a near
///   miss on the observation count.
///
/// Returns the site, the signup goal and today's first visitor on /.
async fn noticed(harness: &Harness, owner: &str) -> (String, String, String) {
    // Daily pseudonyms exist only on their day, so the clock walks through the history.
    let clock = harness.state.collector.clock();
    let site = harness.site("Europe/Vienna").await;
    let mut n = 0;
    let mut ip = move || {
        n += 1;
        format!("198.18.{}.{}", n / 200, n % 200 + 1)
    };
    let ten = |tuesday: i64| tuesday - 5 * HOUR_MS - 20 * MINUTE_MS;
    for week in (1..=4).rev() {
        let at = ten(TUESDAY_1520 - week * WEEK_MS);
        clock.set(at + HOUR_MS);
        day_of_traffic(harness, &site, &mut ip, at, (15, 9), 10, (10, 10)).await;
    }
    clock.set(TUESDAY_1520);
    let home = day_of_traffic(
        harness,
        &site,
        &mut ip,
        ten(TUESDAY_1520),
        (60, 30),
        29,
        (15, 14),
    )
    .await;

    for (back, lcp) in [(8, 2000), (7, 2400), (6, 1800), (5, 2200)] {
        let noon = TUESDAY_1520 - back * DAY_MS - 3 * HOUR_MS - 20 * MINUTE_MS;
        let phone = json!({ "lcp": lcp, "fcp": lcp });
        send(harness, &site, &ip(), PHONE, noon, "performance", phone).await;
        let desktop = json!({ "lcp": 1000 });
        send(harness, &site, &ip(), DESKTOP, noon, "performance", desktop).await;
    }
    let eleven = TUESDAY_1520 - 4 * HOUR_MS - 20 * MINUTE_MS;
    for _ in 0..100 {
        let phone = json!({ "lcp": 2500, "fcp": 2400 });
        send(harness, &site, &ip(), PHONE, eleven, "performance", phone).await;
    }
    for _ in 0..99 {
        let desktop = json!({ "lcp": 9000 });
        send(
            harness,
            &site,
            &ip(),
            DESKTOP,
            eleven,
            "performance",
            desktop,
        )
        .await;
    }

    let mut signup = String::new();
    for name in ["signup", "newsletter"] {
        let definition = json!({
            "predicate": predicate("g", "event_name", name), "scope": "session", "counting": "every"
        });
        let saved = command(
            harness,
            owner,
            "goals.save",
            json!({ "site": site, "name": name, "definition": definition }),
        )
        .await;
        if name == "signup" {
            signup = saved["goal"]["id"].as_str().unwrap().to_owned();
        }
    }
    let workspace = harness.workspaces["ws_a"].as_str().to_owned();
    let board = command(
        harness,
        owner,
        "boards.create",
        json!({ "workspace": workspace, "name": "Shop" }),
    )
    .await;
    for (title, entry) in [("Sale to buy", "/sale"), ("Near to buy", "/near")] {
        let pin =
            json!({ "board": board["board"]["id"], "title": title, "state": funnel(&site, entry) });
        command(harness, owner, "boards.pin", pin).await;
    }
    settle(harness).await;
    (site, signup, home)
}

/// Runs each finding's stored state and checks it shows the finding's values.
async fn shows_its_values(harness: &Harness, cookie: &str, finding: &Value) {
    let state = finding["state"].to_string();
    let result = ok(&harness
        .query(
            Some(cookie),
            "analytics.explore",
            &format!("state={}", encode(&state)),
        )
        .await);
    let values = &finding["values"];
    match values["kind"].as_str().unwrap() {
        "dimension_mover" => {
            assert_eq!(
                result["main"]["totals"][0]["v"], values["today"],
                "{finding}"
            );
            let typical = &result["main"]["compare"][0]["previous"]["v"];
            assert_eq!(typical, &values["typical"], "{finding}");
        }
        "goal_pace" => {
            let goal = &result["report"]["goals"][0];
            assert_eq!(goal["completions"], values["today"]);
            assert_eq!(
                goal["compare"]["completions"]["previous"]["v"],
                values["typical"]
            );
        }
        "funnel_step" => {
            let step = &result["report"]["steps"][values["step"].as_u64().unwrap() as usize - 1];
            assert_eq!(step["reached"], values["entered"]);
            assert_eq!(step["drop"], values["dropped"]);
        }
        "performance" => assert_eq!(result["main"]["totals"][0], values["value"]),
        kind => panic!("unknown kind {kind}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_family_fires_on_its_fixture_ranks_and_shows_its_values() {
    // The clock only moves forward; the fixture starts before its history.
    let h = harness(test_budgets(), TUESDAY_1520 - 5 * WEEK_MS).await;
    let owner = h.session("p_owner").await;
    let (site, signup, home) = noticed(&h, &owner).await;
    let mut schedule = Schedule::default();
    assert!(written(&round(&h, &mut schedule).await, &site));

    let list = findings_of(&h, &owner, &site).await;
    assert_eq!(
        (&list["computed_at"], &list["stale"]),
        (&json!(TUESDAY_1520), &json!(false))
    );
    let findings = list["findings"].as_array().unwrap();
    let order: Vec<(i64, &str)> = findings
        .iter()
        .map(|f| {
            (
                f["rank"].as_i64().unwrap(),
                f["values"]["kind"].as_str().unwrap(),
            )
        })
        .collect();
    let mut expected = vec![(1, "goal_pace"), (2, "performance")];
    expected.extend((3..=7).map(|rank| (rank, "dimension_mover")));
    expected.push((8, "funnel_step"));
    assert_eq!(order, expected, "{list}");
    assert_eq!(
        summaries_of(&h, &owner, &[&site]).await[0]["finding_count"],
        8
    );

    let goal = &findings[0];
    assert_eq!(goal["rule"], "goal_pace");
    assert_eq!(goal["rule_version"], 1);
    assert_eq!(
        goal["values"],
        json!({ "kind": "goal_pace", "goal": signup, "goal_version": 1, "today": 15,
                "typical": 10, "relative_change": 0.5 })
    );
    assert_eq!(
        findings[1]["values"],
        json!({ "kind": "performance", "metric": "lcp", "device": { "t": "string", "v": "mobile" },
                "value": { "t": "millis", "v": 2500 }, "observations": 100,
                "slowest": { "t": "millis", "v": 2400 }, "slowest_day": "2026-10-06", "days": 4 })
    );
    // Yesterday from 15:00 CEST on.
    assert_eq!(
        findings[1]["window_start"],
        TUESDAY_1520 - DAY_MS - 20 * MINUTE_MS
    );
    let movers: Vec<(&Value, &Value, &Value)> = findings[2..7]
        .iter()
        .map(|f| {
            (
                &f["values"]["dimension"],
                &f["values"]["today"],
                &f["share"],
            )
        })
        .collect();
    assert_eq!(
        movers,
        vec![
            (&json!("referrer_domain"), &json!(109), &json!(1.0)),
            (&json!("browser"), &json!(109), &json!(1.0)),
            (&json!("os"), &json!(109), &json!(1.0)),
            (&json!("device"), &json!(109), &json!(1.0)),
            (&json!("path"), &json!(60), &json!(0.7031)),
        ]
    );
    assert_eq!(
        findings[6]["values"],
        json!({ "kind": "dimension_mover", "dimension": "path", "value": { "t": "string", "v": "/sale" },
                "today": 60, "typical": 15, "ratio": 4.0, "site_today": 109, "site_typical": 45 })
    );
    let step = &findings[7]["values"];
    assert_eq!(
        (&step["title"], &step["step"], &step["until_hour"]),
        (&json!("Sale to buy"), &json!(1), &json!(15))
    );
    assert_eq!(
        [
            &step["entered"],
            &step["dropped"],
            &step["baseline_entered"],
            &step["baseline_dropped"]
        ],
        [&json!(60), &json!(30), &json!(60), &json!(24)]
    );
    assert_eq!(
        (
            &step["drop_off"],
            &step["baseline_drop_off"],
            &step["baseline_days"]
        ),
        (&json!(0.5), &json!(0.4), &json!(4))
    );
    for finding in findings {
        assert_eq!(finding["computed_at"], TUESDAY_1520);
        shows_its_values(&h, &owner, finding).await;
    }

    // The next run replaces the set: the signup goal is archived and the newsletter reaches
    // 15. The hourly rules keep their findings from the first run.
    command(
        &h,
        &owner,
        "goals.archive",
        json!({ "site": site, "goal": signup }),
    )
    .await;
    let at = TUESDAY_1520 - MINUTE_MS;
    let newsletter = json!({ "name": "newsletter" });
    send(&h, &site, &home, DESKTOP, at, "event", newsletter.clone()).await;
    settle(&h).await;
    let clock = h.state.collector.clock();
    clock.set(TUESDAY_1520 + 10 * MINUTE_MS);
    assert!(written(&round(&h, &mut schedule).await, &site));
    let list = findings_of(&h, &owner, &site).await;
    let findings = list["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 8);
    assert_eq!(findings[0]["values"]["today"], 15);
    assert_ne!(findings[0]["values"]["goal"], json!(signup));
    let computed: Vec<(&str, &Value)> = findings
        .iter()
        .map(|f| (f["rule"].as_str().unwrap(), &f["computed_at"]))
        .collect();
    for (rule, at) in computed {
        let hourly = rule == "performance" || rule == "funnel_step";
        let expected = if hourly {
            TUESDAY_1520
        } else {
            TUESDAY_1520 + 10 * MINUTE_MS
        };
        assert_eq!(at, &json!(expected), "{rule}");
    }

    // An hour after their run they run again.
    send(&h, &site, &home, DESKTOP, at, "event", newsletter).await;
    settle(&h).await;
    clock.set(TUESDAY_1520 + HOUR_MS);
    assert!(written(&round(&h, &mut schedule).await, &site));
    let list = findings_of(&h, &owner, &site).await;
    assert!(
        list["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["computed_at"] == TUESDAY_1520 + HOUR_MS),
        "{list}"
    );
}
