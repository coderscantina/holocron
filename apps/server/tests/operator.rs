//! `operator.status` through the real router: install admins only, the storage forecast from
//! fixture samples at a pinned clock, and samples read from `day_aggregates` and file sizes.

mod common;

use holocron_server::clock::Clock;
use holocron_server::config::CollectConfig;
use holocron_server::maintenance::samples;
use serde_json::json;

use common::{Harness, test_budgets};

/// 2026-10-15T09:00:00Z.
const NOW_MS: i64 = 1_792_054_800_000;
const DAY_MS: i64 = 86_400_000;
const GB: i64 = 1_000_000_000;

async fn pinned() -> Harness {
    Harness::with(
        CollectConfig::default(),
        test_budgets(),
        Clock::frozen(NOW_MS),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn operator_status_is_for_install_admins_only() {
    let harness = Harness::new().await;
    // p_owner owns workspaces but is not an install admin.
    for alias in ["p_owner", "p_member", "p_viewer"] {
        let cookie = harness.session(alias).await;
        let answer = harness.query(Some(&cookie), "operator.status", "").await;
        assert_eq!(
            (answer.status.as_u16(), answer.code()),
            (403, "forbidden".to_owned()),
            "{alias}"
        );
    }
    assert_eq!(harness.query(None, "operator.status", "").await.status, 401);
    let admin = harness.session("p_install_admin").await;
    assert_eq!(
        harness
            .query(Some(&admin), "operator.status", "")
            .await
            .status,
        200
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_forecast_comes_from_the_latest_sample_and_names_no_site() {
    let harness = pinned().await;
    // Two days ago and today: 100M records in 10 GB, 300,000 a day since 2025-11-20.
    for (taken_at, free) in [(NOW_MS - 2 * DAY_MS, 61 * GB), (NOW_MS, 60 * GB)] {
        sqlx::query(
            "INSERT INTO storage_samples VALUES (?, 50000000, 50000000, ?, ?, 0, ?, 100000000, 300000, ?)",
        )
        .bind(taken_at)
        .bind(5 * GB)
        .bind(10 * GB)
        .bind(free)
        .bind(20_412) // 2025-11-20
        .execute(&harness.state.db.writer)
        .await
        .unwrap();
    }
    let admin = harness.session("p_install_admin").await;
    let answer = harness.query(Some(&admin), "operator.status", "").await;
    assert_eq!(answer.status, 200);
    let status = answer.json();

    // 100 × 300,000 × 365 = 10,950,000,000 + 100,000,000 SQLite + 25 GiB replay cap
    // (26,843,545,600) + no reserve = 37,893,545,600, under 15.1 + 60 GB.
    assert_eq!(status["recording"]["cap_bytes"], 26_843_545_600_i64);
    assert_eq!(
        status["forecast"]["outcome"],
        json!({ "kind": "levels_off", "size_bytes": 37_893_545_600_i64, "on": "2026-11-20" })
    );
    assert_eq!(status["forecast"]["inputs"]["sampled_at"], NOW_MS);
    assert_eq!(
        status["forecast"]["inputs"]["days_until_full_retention"],
        36
    );
    assert_eq!(status["samples"].as_array().unwrap().len(), 2);
    assert_eq!(status["samples"][1]["used_bytes"], 15_100_000_000_i64);
    assert_eq!(status["problems"], json!([]));

    let text = status.to_string();
    for site in harness.sites.values() {
        assert!(!text.contains(site.as_str()), "site {site:?} in the status");
    }
    for workspace in harness.workspaces.values() {
        assert!(
            !text.contains(workspace.as_str()),
            "workspace in the status"
        );
    }
    assert!(!text.contains("example.test"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sample_takes_retained_records_and_median_ingest_from_day_aggregates() {
    let harness = pinned().await;
    // Ten days ending yesterday (2026-10-14): 100, 200, …, 1000 eligible events; site 2 adds
    // 50 on the last day. The 28-day window starts at the oldest day, so its median is the
    // lower middle of 100…900 and 1050: 500.
    harness
        .state
        .collector
        .storage()
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO day_aggregates
                 SELECT 1, DATE '2026-10-05' + i::INTEGER, 1, [], 0, 0, (i + 1) * 100, 0, 0, 0, 0, 0, 0, 0, 1
                 FROM range(10) t(i);
                 INSERT INTO day_aggregates VALUES
                     (2, DATE '2026-10-14', 1, [], 0, 0, 50, 0, 0, 0, 0, 0, 0, 0, 1);",
            )
        })
        .await
        .unwrap();

    let sample = samples::take(&harness.state).await.unwrap();
    assert_eq!(sample.taken_at, NOW_MS);
    assert_eq!(sample.retained_records, 5_550);
    assert_eq!(sample.ingest_per_day, 500);
    assert_eq!(sample.oldest_day, Some(20_731)); // 2026-10-05
    assert!(sample.analytics_bytes > 0 && sample.control_bytes > 0);

    let forecast = samples::forecast(&sample, 0, 0);
    assert_eq!(
        serde_json::to_value(&forecast.outcome).unwrap(),
        json!({ "kind": "not_enough_history", "days": 10 })
    );
    assert_eq!(
        samples::list(&harness.state.db.reader).await.unwrap(),
        vec![sample]
    );
}
