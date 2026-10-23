//! Session derivation and correction through real admission, transfer and DuckDB.

mod common;

use chrono::NaiveDate;
use common::*;
use holocron_analytics::explore::session::{session, timeline};
use holocron_analytics::identity::{IdentityDay, SessionPolicy};
use holocron_analytics::traffic::{Range, traffic};
use holocron_analytics::{Correction, Limits, freshness, jobs};
use holocron_protocol::exploration::Cell;
use holocron_storage::EventKind::{CustomEvent, Pageview, Performance};
use holocron_storage::{
    HeatmapInteraction, Identify, Interaction, Lane, Property, PropertyValue, Record,
    ReplayFragment, Storage,
};

const SITE: u128 = 0x0d2c_9a6e_5a3b_4f1e_9b1c_7e2f_1a4d_8c01;

fn vienna() -> Site {
    Site::new(SITE, chrono_tz::Europe::Vienna)
}

fn ids(records: &[&Record]) -> Vec<u128> {
    records.iter().map(|r| id_of(r)).collect()
}

/// `[start, end)` of one local date of the site.
fn day_range(site: &Site, date: &str) -> Range {
    let date: NaiveDate = date.parse().unwrap();
    let day = |date| IdentityDay { epoch: 1, date };
    Range {
        start_ms: site.policies.day_end_ms(day(date.pred_opt().unwrap())),
        end_ms: site.policies.day_end_ms(day(date)),
    }
}

async fn read_traffic(
    storage: &Storage,
    site: &Site,
    range: Range,
) -> holocron_analytics::traffic::Traffic {
    let id = site.id;
    storage
        .read(Lane::Report, move |s| {
            traffic(s, id, range, chrono_tz::Europe::Vienna)
        })
        .await
        .unwrap()
        .value
}

#[tokio::test(flavor = "multi_thread")]
async fn late_event_extends_a_session() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let a = site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1"));
    let b = site.event(Pageview, "2026-06-02T10:10:00Z", Who::Anon("v1"));
    ingest(&storage, &[a.clone(), b.clone()]).await;
    correct(&storage).await;
    let late = site.event_at(
        Pageview,
        ms("2026-06-02T10:35:00Z"),
        ms("2026-06-03T09:00:00Z"),
        Who::Anon("v1"),
    );
    ingest(&storage, std::slice::from_ref(&late)).await;
    correct(&storage).await;
    let sessions = sessions(&storage, SITE).await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].members, ids(&[&a, &b, &late]));
    assert_eq!(sessions[0].span_ms, 35 * 60_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn late_event_joins_two_sessions() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let a = site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1"));
    let b = site.event(Pageview, "2026-06-02T10:40:00Z", Who::Anon("v1"));
    ingest(&storage, &[a.clone(), b.clone()]).await;
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await, vec![ids(&[&a]), ids(&[&b])]);
    let late = site.event_at(
        CustomEvent,
        ms("2026-06-02T10:20:00Z"),
        ms("2026-06-02T18:00:00Z"),
        Who::Anon("v1"),
    );
    ingest(&storage, std::slice::from_ref(&late)).await;
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await, vec![ids(&[&a, &late, &b])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn late_revocation_splits_a_profile_session() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let a = site.event(Pageview, "2026-08-11T09:00:00Z", Who::Profile("dora"));
    let b = site.event(Pageview, "2026-08-11T09:10:00Z", Who::Profile("dora"));
    ingest(&storage, &[a.clone(), b.clone()]).await;
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await, vec![ids(&[&a, &b])]);
    // Arrives after both events; consent was revoked and granted again between them.
    let revoked = site.revocation("dora", "2026-08-11T09:05:00Z");
    ingest(&storage, &[revoked]).await;
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await, vec![ids(&[&a]), ids(&[&b])]);
}

#[tokio::test(flavor = "multi_thread")]
async fn window_change_splits_sessions_from_its_effective_instant() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    jobs::record_policies(&storage, SITE, site.policies.clone())
        .await
        .unwrap();
    let early = [
        site.event(Pageview, "2026-06-02T08:00:00Z", Who::Anon("v1")),
        site.event(Pageview, "2026-06-02T08:20:00Z", Who::Anon("v1")),
    ];
    let later = [
        site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1")),
        site.event(Pageview, "2026-06-02T10:20:00Z", Who::Anon("v1")),
    ];
    ingest(&storage, &[early.clone(), later.clone()].concat()).await;
    correct(&storage).await;
    assert_eq!(
        sessions(&storage, SITE)
            .await
            .iter()
            .map(|s| s.policy)
            .collect::<Vec<_>>(),
        [1, 1]
    );

    let v2 = site
        .policies
        .change(
            SessionPolicy::new(chrono_tz::Europe::Vienna, 15).unwrap(),
            ms("2026-06-02T09:00:00Z"),
            ms("2026-06-02T12:00:00Z"),
        )
        .unwrap();
    assert_eq!(v2.version, 2);
    assert!(
        jobs::record_policies(&storage, SITE, site.policies.clone())
            .await
            .unwrap()
    );
    correct(&storage).await;
    let sessions = sessions(&storage, SITE).await;
    let shape: Vec<(Vec<u128>, i32)> = sessions
        .into_iter()
        .map(|s| (s.members, s.policy))
        .collect();
    assert_eq!(
        shape,
        [
            (ids(&[&early[0], &early[1]]), 1),
            (ids(&[&later[0]]), 2),
            (ids(&[&later[1]]), 2),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn recorded_policies_follow_their_owner_and_rebuild_what_changed() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let owner_v1 = site.policies.clone();
    let events = [
        site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1")),
        site.event(Pageview, "2026-06-02T10:20:00Z", Who::Anon("v1")),
    ];
    ingest(&storage, &events).await;
    assert!(
        jobs::record_policies(&storage, SITE, owner_v1.clone())
            .await
            .unwrap()
    );
    assert!(
        !jobs::record_policies(&storage, SITE, owner_v1.clone())
            .await
            .unwrap(),
        "unchanged"
    );
    correct(&storage).await;

    let window = SessionPolicy::new(chrono_tz::Europe::Vienna, 15).unwrap();
    site.policies
        .change(
            window,
            ms("2026-06-02T09:00:00Z"),
            ms("2026-06-02T09:00:00Z"),
        )
        .unwrap();
    assert!(
        jobs::record_policies(&storage, SITE, site.policies.clone())
            .await
            .unwrap()
    );
    correct(&storage).await;
    let policies = |s: Vec<Session>| s.iter().map(|s| s.policy).collect::<Vec<_>>();
    assert_eq!(policies(sessions(&storage, SITE).await), [2, 2]);

    // The owner rolled the change back: the copy drops version 2 and the day is rebuilt.
    assert!(
        jobs::record_policies(&storage, SITE, owner_v1)
            .await
            .unwrap()
    );
    correct(&storage).await;
    assert_eq!(policies(sessions(&storage, SITE).await), [1]);
}

#[tokio::test(flavor = "multi_thread")]
async fn anonymous_sessions_close_at_local_midnight_and_profiles_cross_it() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    // 23:50 and 00:05 local.
    let anon = [
        site.event(Pageview, "2026-06-16T21:50:00Z", Who::Anon("f5")),
        site.event(Pageview, "2026-06-16T22:05:00Z", Who::Anon("f5")),
    ];
    let profile = [
        site.event(Pageview, "2026-06-16T21:50:00Z", Who::Profile("ben")),
        site.event(Pageview, "2026-06-16T22:05:00Z", Who::Profile("ben")),
    ];
    // Even a pseudonym that repeated across the day boundary is not linked across days.
    let mut repeated = [
        site.event(Pageview, "2026-06-17T21:55:00Z", Who::Anon("r")),
        site.event(Pageview, "2026-06-17T22:01:00Z", Who::Anon("r")),
    ];
    for record in &mut repeated {
        if let Record::Event(e) = record {
            e.daily_id = Some(pseudonym("one value on two days"));
        }
    }
    ingest(
        &storage,
        &[anon.clone(), profile.clone(), repeated.clone()].concat(),
    )
    .await;
    correct(&storage).await;
    let mut expected = vec![
        ids(&[&anon[0]]),
        ids(&[&anon[1]]),
        ids(&[&profile[0], &profile[1]]),
        ids(&[&repeated[0]]),
        ids(&[&repeated[1]]),
    ];
    expected.sort();
    assert_eq!(groups(&storage, SITE).await, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn gaps_are_measured_in_instants_across_dst() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let fall = [
        site.event(Pageview, "2025-10-26T00:40:00Z", Who::Anon("v2")), // 02:40 CEST
        site.event(Pageview, "2025-10-26T01:05:00Z", Who::Anon("v2")), // 02:05 CET
        site.event(Pageview, "2025-10-26T22:55:00Z", Who::Anon("v2")), // 23:55, 25-hour day
        site.event(Pageview, "2025-10-26T23:05:00Z", Who::Anon("v2")), // 00:05 next day
    ];
    let spring = [
        site.event(Pageview, "2026-03-29T00:50:00Z", Who::Anon("v1")), // 01:50 CET
        site.event(Pageview, "2026-03-29T01:10:00Z", Who::Anon("v1")), // 03:10 CEST
        site.event(Pageview, "2026-03-29T21:50:00Z", Who::Anon("v1")), // 23:50, 23-hour day
        site.event(Pageview, "2026-03-29T22:05:00Z", Who::Anon("v1")), // 00:05 next day
    ];
    ingest(&storage, &[fall.clone(), spring.clone()].concat()).await;
    correct(&storage).await;
    let mut expected = vec![
        ids(&[&fall[0], &fall[1]]),
        ids(&[&fall[2]]),
        ids(&[&fall[3]]),
        ids(&[&spring[0], &spring[1]]),
        ids(&[&spring[2]]),
        ids(&[&spring[3]]),
    ];
    expected.sort();
    assert_eq!(groups(&storage, SITE).await, expected);
    let spans: Vec<i64> = sessions(&storage, SITE)
        .await
        .iter()
        .map(|s| s.span_ms)
        .filter(|&s| s > 0)
        .collect();
    assert_eq!(spans.iter().sum::<i64>(), 1_500_000 + 1_200_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn inactivity_window_is_half_open_and_one_event_sessions_have_zero_span() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let first = site.event(Pageview, "2026-06-02T08:00:00Z", Who::Anon("v3"));
    let exactly_window = site.event(Pageview, "2026-06-02T08:30:00Z", Who::Anon("v3"));
    let just_under = site.event(Pageview, "2026-06-02T08:59:59.999Z", Who::Anon("v3"));
    ingest(
        &storage,
        &[first.clone(), exactly_window.clone(), just_under.clone()],
    )
    .await;
    correct(&storage).await;
    let sessions = sessions(&storage, SITE).await;
    let shape: Vec<(Vec<u128>, i64)> = sessions
        .into_iter()
        .map(|s| (s.members, s.span_ms))
        .collect();
    assert_eq!(
        shape,
        [
            (ids(&[&first]), 0),
            (ids(&[&exactly_window, &just_under]), 1_799_999)
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn timezone_change_rotates_identity_and_versions_sessions() {
    let (_dir, storage) = open().await;
    let mut site = Site::new(SITE, chrono_tz::America::New_York);
    let change = ms("2026-09-15T12:00:00Z");
    let utc = SessionPolicy::new(chrono_tz::UTC, 30).unwrap();
    site.policies.change(utc, change, change).unwrap();
    jobs::record_policies(&storage, SITE, site.policies.clone())
        .await
        .unwrap();

    // Same inputs 20 minutes apart, both on local date 09-15, either side of the change.
    let anon = [
        site.event(Pageview, "2026-09-15T11:50:00Z", Who::Anon("b")),
        site.event(Pageview, "2026-09-15T12:10:00Z", Who::Anon("b")),
    ];
    let profile = [
        site.event(Pageview, "2026-09-15T11:50:00Z", Who::Profile("eve")),
        site.event(Pageview, "2026-09-15T12:10:00Z", Who::Profile("eve")),
    ];
    ingest(&storage, &[anon.clone(), profile.clone()].concat()).await;
    correct(&storage).await;
    let mut shape: Vec<(Vec<u128>, i32)> = sessions(&storage, SITE)
        .await
        .into_iter()
        .map(|s| (s.members, s.policy))
        .collect();
    shape.sort();
    let mut expected = vec![
        (ids(&[&anon[0]]), 1),
        (ids(&[&anon[1]]), 2),
        (ids(&[&profile[0], &profile[1]]), 1),
    ];
    expected.sort();
    assert_eq!(shape, expected);

    let versions = storage
        .read(Lane::Report, move |s| {
            freshness(
                s,
                SITE,
                ms("2026-09-15T00:00:00Z"),
                ms("2026-09-16T00:00:00Z"),
            )
        })
        .await
        .unwrap()
        .value
        .session_policy_versions;
    assert_eq!(versions, [1, 2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn identified_mode_transitions_never_merge_history() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let anonymous = site.event(Pageview, "2026-09-15T08:00:00Z", Who::Anon("anna-phone"));
    let identified = site.event(Pageview, "2026-09-15T08:05:00Z", Who::DailyId("anna"));
    let consented = site.event(Pageview, "2026-09-15T08:10:00Z", Who::Profile("anna"));
    let after_revocation = site.event(Pageview, "2026-09-15T08:20:00Z", Who::DailyId("anna"));
    let records = [
        anonymous.clone(),
        identified.clone(),
        consented.clone(),
        after_revocation.clone(),
    ];
    ingest(&storage, &records).await;
    correct(&storage).await;

    let sessions = sessions(&storage, SITE).await;
    let mut shape: Vec<(Vec<u128>, u8)> = sessions
        .into_iter()
        .map(|s| (s.members, s.population))
        .collect();
    shape.sort();
    let mut expected = vec![
        (ids(&[&anonymous]), 1),
        (ids(&[&identified, &after_revocation]), 1),
        (ids(&[&consented]), 2),
    ];
    expected.sort();
    assert_eq!(shape, expected);

    let t = read_traffic(&storage, &site, day_range(&site, "2026-09-15")).await;
    // The anonymous phone and anna's one daily identified pseudonym, linked or not.
    assert_eq!(
        (t.visitors, t.identified_people, t.linked_events),
        (2, 1, 1)
    );
}

/// m08 saw `count(DISTINCT profile_id)` return 1 for two profiles on DuckDB 1.5.6.
#[tokio::test(flavor = "multi_thread")]
async fn identified_people_counts_two_profiles_as_two() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let records = [
        site.event(Pageview, "2026-06-02T10:00:00Z", Who::Profile("anna")),
        site.event(Pageview, "2026-06-02T10:05:00Z", Who::Profile("ben")),
        site.event(CustomEvent, "2026-06-02T10:10:00Z", Who::Profile("anna")),
        site.event(Pageview, "2026-06-03T09:00:00Z", Who::Profile("ben")),
        site.event(Pageview, "2026-06-02T10:15:00Z", Who::Anon("v1")),
    ];
    ingest(&storage, &records).await;
    correct(&storage).await;
    let day = read_traffic(&storage, &site, day_range(&site, "2026-06-02")).await;
    assert_eq!((day.identified_people, day.linked_events), (2, 3));
    let range = Range {
        start_ms: day_range(&site, "2026-06-02").start_ms,
        end_ms: day_range(&site, "2026-06-03").end_ms,
    };
    let both = read_traffic(&storage, &site, range).await;
    assert_eq!(both.identified_people, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn consent_revocation_keeps_history_and_closes_the_profile_session() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let linked = site.event(Pageview, "2026-08-11T09:00:00Z", Who::Profile("dora"));
    let revoked = site.revocation("dora", "2026-08-11T09:05:00Z");
    let unlinked = site.event(Pageview, "2026-08-11T09:06:00Z", Who::DailyId("dora"));
    let regranted = site.event(Pageview, "2026-08-11T09:12:00Z", Who::Profile("dora"));
    ingest(
        &storage,
        &[linked.clone(), revoked, unlinked.clone(), regranted.clone()],
    )
    .await;
    correct(&storage).await;
    let mut shape: Vec<(Vec<u128>, u8)> = sessions(&storage, SITE)
        .await
        .into_iter()
        .map(|s| (s.members, s.population))
        .collect();
    shape.sort();
    let mut expected = vec![
        (ids(&[&linked]), 2),
        (ids(&[&unlinked]), 1),
        (ids(&[&regranted]), 2),
    ];
    expected.sort();
    assert_eq!(shape, expected);

    let t = read_traffic(&storage, &site, day_range(&site, "2026-08-11")).await;
    assert_eq!(
        (t.visitors, t.identified_people, t.linked_events),
        (1, 1, 2)
    );
}

fn identify(site: &Site, id: u128, at: &str, who: &str) -> Record {
    let time = ms(at);
    let day = site.policies.identity_day(time);
    Record::Identify(Identify {
        id,
        site: site.id,
        event_time_ms: time,
        receive_time_ms: time,
        local_day: day.local_day(),
        daily_id: Some(pseudonym(&format!(
            "anon {who} {} epoch {}",
            day.date, day.epoch
        ))),
        profile_id: None,
        properties: vec![Property {
            key: "plan".into(),
            value: PropertyValue::String("pro".into()),
        }],
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn performance_and_identify_attach_without_moving_boundaries() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let a = site.event(Pageview, "2026-06-02T08:30:00Z", Who::Anon("v3"));
    let b = site.event(Pageview, "2026-06-02T08:59:59.999Z", Who::Anon("v3"));
    let in_tail = site.event(Performance, "2026-06-02T09:20:00Z", Who::Anon("v3"));
    let after_tail = site.event(Performance, "2026-06-02T09:31:00Z", Who::Anon("v3"));
    let before = identify(&site, 7, "2026-06-02T07:00:00Z", "v3");
    ingest(
        &storage,
        &[a.clone(), b.clone(), in_tail.clone(), after_tail, before],
    )
    .await;
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await, vec![ids(&[&a, &b])]);

    let attached = storage
        .read(Lane::Report, |s| {
            let mut st = s.connection().prepare(
                "SELECT m.role, coalesce(e.id, 7::UHUGEINT) FROM session_members m
                 LEFT JOIN events e ON e.seq = m.seq WHERE m.role <> 1 ORDER BY m.role",
            )?;
            st.query_map([], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, u128>(1)?)))?
                .collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value;
    assert_eq!(attached, [(2, id_of(&in_tail)), (3, 7)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn keyless_events_count_without_forming_sessions() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let records = [
        site.event(Pageview, "2026-06-02T12:00:00Z", Who::Anon("v6")),
        site.event_at(
            Pageview,
            ms("2026-06-02T12:05:00Z"),
            ms("2026-06-05T10:00:00Z"),
            Who::Keyless,
        ),
        site.event_at(
            CustomEvent,
            ms("2026-06-02T12:06:00Z"),
            ms("2026-06-05T10:00:00Z"),
            Who::Keyless,
        ),
    ];
    ingest(&storage, &records).await;
    correct(&storage).await;
    let t = read_traffic(&storage, &site, day_range(&site, "2026-06-02")).await;
    assert_eq!(
        (t.pageviews, t.events, t.sessions, t.visitors),
        (2, 1, 1, 1)
    );
    assert_eq!((t.identified_events, t.eligible_events), (1, 3));
}

async fn capture_rows(storage: &Storage) -> Vec<(i64, u128, u16, String)> {
    storage
        .read(Lane::Report, |s| {
            let mut st = s.connection().prepare(
                "SELECT seq, capture_id, chunk_index, md5(payload) FROM replay_fragments
                 UNION ALL
                 SELECT seq, capture_id, 0, md5(path || kind::VARCHAR) FROM heatmap_interactions
                 ORDER BY 1",
            )?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap()
        .value
}

#[tokio::test(flavor = "multi_thread")]
async fn recalculation_leaves_capture_ids_and_chunk_order_intact() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let at = ms("2026-06-02T10:00:00Z");
    let mut records = vec![
        site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1")),
        site.event(Pageview, "2026-06-02T10:40:00Z", Who::Anon("v1")),
    ];
    for index in 0..3u16 {
        records.push(Record::ReplayFragment(ReplayFragment {
            id: 100 + u128::from(index),
            site: SITE,
            capture_id: 42,
            index,
            total: 3,
            is_final: index == 2,
            event_time_ms: at + i64::from(index),
            receive_time_ms: at,
            payload: vec![b'a' + index as u8; 1024],
        }));
    }
    records.push(Record::HeatmapInteraction(HeatmapInteraction {
        id: 200,
        site: SITE,
        capture_id: 42,
        event_time_ms: at,
        receive_time_ms: at,
        path: "/".into(),
        viewport_width: 390,
        viewport_height: 844,
        kind: Interaction::Click { x: 10, page_y: 20 },
    }));
    ingest(&storage, &records).await;
    correct(&storage).await;
    let before = capture_rows(&storage).await;

    // A late event joins the two sessions, then the whole day is recalculated.
    let late = site.event_at(
        Pageview,
        ms("2026-06-02T10:20:00Z"),
        ms("2026-06-02T20:00:00Z"),
        Who::Anon("v1"),
    );
    ingest(&storage, &[late]).await;
    let day: NaiveDate = "2026-06-02".parse().unwrap();
    jobs::recalculate(&storage, SITE, day..=day).await.unwrap();
    correct(&storage).await;
    assert_eq!(groups(&storage, SITE).await.len(), 1);
    assert_eq!(capture_rows(&storage).await, before);
}

/// Every derived row except the batch it was computed at.
async fn derived(storage: &Storage) -> Vec<String> {
    storage
        .read(Lane::Report, |s| {
            let mut st = s.connection().prepare(
                "SELECT 'session ' || s::VARCHAR FROM sessions s
                 UNION ALL SELECT 'member ' || seq || ' ' || session_id || ' ' || role || ' ' || event_time FROM session_members
                 UNION ALL SELECT 'day ' || local_day || ' ' || metric_version || ' ' || session_policy_versions::VARCHAR || ' ' ||
                        pageviews || ' ' || custom_events || ' ' || eligible_events || ' ' || identified_events || ' ' ||
                        linked_events || ' ' || visitors || ' ' || sessions || ' ' || pageview_sessions || ' ' ||
                        bounces || ' ' || span_sum_ms FROM day_aggregates
                 UNION ALL SELECT 'hour ' || local_day || ' ' || hour || ' ' || pageviews || ' ' || custom_events || ' ' ||
                        visitors FROM hour_aggregates
                 UNION ALL SELECT 'dimension ' || local_day || ' ' || field || ' ' || coalesce(value, '-') || ' ' ||
                        pageviews || ' ' || custom_events || ' ' || visitors FROM dimension_aggregates
                 UNION ALL SELECT 'visitor ' || local_day || ' ' || daily_id || ' ' || first_time || ' ' || last_time
                        FROM visitor_days
                 ORDER BY 1",
            )?;
            st.query_map([], |r| r.get(0))?.collect::<holocron_storage::duckdb::Result<Vec<String>>>()
        })
        .await
        .unwrap()
        .value
}

fn mixed_records(site: &mut Site) -> Vec<Record> {
    let mut out = Vec::new();
    for (day, hour) in [("2026-06-02", 8), ("2026-06-03", 21), ("2026-06-04", 9)] {
        for (minute, who) in [
            (0, "a"),
            (5, "b"),
            (25, "a"),
            (31, "b"),
            (50, "a"),
            (59, "c"),
        ] {
            let at = format!("{day}T{hour:02}:{minute:02}:00Z");
            out.push(site.event(Pageview, &at, Who::Anon(who)));
            out.push(site.event(CustomEvent, &at, Who::Profile("p")));
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn correction_is_idempotent_and_replay_is_deterministic() {
    let mut site = vienna();
    let records = mixed_records(&mut site);

    let (_a, waves) = open().await;
    for wave in records.chunks(7) {
        ingest(&waves, wave).await;
        correct(&waves).await;
    }
    let (_b, once) = open().await;
    ingest(&once, &records).await;
    correct(&once).await;
    let expected = derived(&once).await;
    assert!(!expected.is_empty());
    assert_eq!(derived(&waves).await, expected);

    let all = "2026-06-01".parse().unwrap()..="2026-06-05".parse().unwrap();
    jobs::recalculate(&waves, SITE, all).await.unwrap();
    correct(&waves).await;
    assert_eq!(derived(&waves).await, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_stay_pending_while_their_days_are_dirty() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    ingest(
        &storage,
        &[site.event(Pageview, "2026-06-02T10:00:00Z", Who::Anon("v1"))],
    )
    .await;
    let state = |range: Range| {
        storage.read(Lane::Report, move |s| {
            freshness(s, SITE, range.start_ms, range.end_ms)
        })
    };
    let dirty = day_range(&site, "2026-06-02");
    let elsewhere = day_range(&site, "2026-07-20");
    assert_eq!(
        state(dirty).await.unwrap().value.correction,
        Correction::Pending { dirty_days: 1 }
    );
    assert_eq!(
        state(elsewhere).await.unwrap().value.correction,
        Correction::Corrected
    );

    correct(&storage).await;
    let read = state(dirty).await.unwrap();
    assert_eq!(read.value.correction, Correction::Corrected);
    assert_eq!(read.value.corrected_through, read.watermark.batch);
}

#[tokio::test(flavor = "multi_thread")]
async fn day_aggregates_match_the_row_level_summary() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    ingest(&storage, &mixed_records(&mut site)).await;
    correct(&storage).await;
    for date in ["2026-06-02", "2026-06-03", "2026-06-04"] {
        let t = read_traffic(&storage, &site, day_range(&site, date)).await;
        let day: NaiveDate = date.parse().unwrap();
        let aggregate = storage
            .read(Lane::Report, move |s| {
                s.connection().query_row(
                    "SELECT pageviews, custom_events, eligible_events, identified_events, linked_events, visitors,
                            sessions, pageview_sessions, bounces, span_sum_ms
                     FROM day_aggregates WHERE site = ? AND local_day = ?",
                    holocron_storage::duckdb::params![SITE, holocron_storage::duckdb::types::Value::Date32(
                        holocron_analytics::identity::days_since_epoch(day)
                    )],
                    |r| (0..10).map(|i| r.get::<_, i64>(i)).collect::<Result<Vec<_>, _>>(),
                )
            })
            .await
            .unwrap()
            .value;
        let summary = [
            t.pageviews,
            t.events,
            t.eligible_events,
            t.identified_events,
            t.linked_events,
            t.visitors,
            t.sessions,
            t.pageview_sessions,
            t.bounces,
        ]
        .map(|n| n as i64);
        assert_eq!(aggregate[..9], summary, "{date}");
        assert_eq!(aggregate[9], t.span_sum_ms, "{date}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn correction_steps_are_bounded() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let records: Vec<Record> = (1..=5)
        .map(|d| {
            site.event(
                Pageview,
                &format!("2026-06-0{d}T10:00:00Z"),
                Who::Anon("v1"),
            )
        })
        .collect();
    ingest(&storage, &records).await;
    let limits = Limits { days: 2 };
    let mut steps = Vec::new();
    loop {
        let step = holocron_analytics::step(&storage, limits).await.unwrap();
        steps.push((step.days, step.remaining));
        if step.remaining == 0 {
            break;
        }
    }
    assert_eq!(steps, [(2, 3), (2, 1), (1, 0)]);
}

fn profile_identify(site: &Site, id: u128, at: &str, who: &str) -> Record {
    let time = ms(at);
    let day = site.policies.identity_day(time);
    Record::Identify(Identify {
        id,
        site: site.id,
        event_time_ms: time,
        receive_time_ms: time,
        local_day: day.local_day(),
        daily_id: Some(pseudonym(&format!(
            "daily-id {who} {} epoch {}",
            day.date, day.epoch
        ))),
        profile_id: Some(pseudonym(&format!("profile {who}"))),
        properties: vec![Property {
            key: "plan".into(),
            value: PropertyValue::String(format!("{who} {at}")),
        }],
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn correction_around_a_middle_day_equals_a_full_rebuild() {
    let mut site = vienna();
    let mut history = Vec::new();
    let mut next_identify = 1_000;
    for d in 1..=20 {
        let at = |time: &str| format!("2026-06-{d:02}T{time}:00Z");
        for who in ["p1", "p2", "p3"] {
            history.push(profile_identify(&site, next_identify, &at("05:00"), who));
            next_identify += 1;
            for time in ["10:00", "10:10", "10:20", "14:00", "14:10"] {
                history.push(site.event(Pageview, &at(time), Who::Profile(who)));
            }
            history.push(site.event(Performance, &at("10:15"), Who::Profile(who)));
            history.push(site.event(CustomEvent, &at("11:05"), Who::Profile(who)));
            // Local midnight is 22:00Z: this session crosses it.
            for time in ["21:45", "21:55", "22:05", "22:15"] {
                history.push(site.event(Pageview, &at(time), Who::Profile(who)));
            }
        }
        history.push(site.event(Pageview, &at("10:00"), Who::Anon("v1")));
        history.push(site.event(Pageview, &at("10:20"), Who::Anon("v1")));
    }
    // Arrives a day after the history, all on 2026-06-10.
    let late = |site: &mut Site, kind, time: &str, who| {
        site.event_at(
            kind,
            ms(&format!("2026-06-10T{time}:00Z")),
            ms("2026-06-21T12:00:00Z"),
            who,
        )
    };
    let changes = vec![
        // Joins the 10:00 and 11:05 sessions.
        late(&mut site, Pageview, "10:45", Who::Profile("p1")),
        // Lands in the tail of the session crossing midnight.
        late(&mut site, Performance, "22:30", Who::Profile("p1")),
        // A new session the earlier identify now attaches to.
        profile_identify(&site, 1, "2026-06-10T03:00:00Z", "p2"),
        late(&mut site, Pageview, "07:00", Who::Profile("p2")),
        // Splits the 14:00 session.
        site.revocation("p3", "2026-06-10T14:05:00Z"),
        late(&mut site, Pageview, "10:40", Who::Anon("v1")),
    ];

    let (_a, bounded) = open().await;
    ingest(&bounded, &history).await;
    correct(&bounded).await;
    ingest(&bounded, &changes).await;
    let mut rebuilt = 0;
    loop {
        let step = holocron_analytics::step(&bounded, Limits::default())
            .await
            .unwrap();
        rebuilt += step.sessions;
        if step.remaining == 0 {
            break;
        }
    }
    // The profiles hold 240 sessions; only those around the changed days are rebuilt.
    assert!(rebuilt < 100, "rebuilt {rebuilt} sessions");

    let all = [history, changes].concat();
    let (_b, once) = open().await;
    ingest(&once, &all).await;
    correct(&once).await;
    let expected = derived(&once).await;
    assert_eq!(derived(&bounded).await, expected);

    // From scratch in steps of three days, so profile sessions are merged across steps.
    let (_c, stepped) = open().await;
    ingest(&stepped, &all).await;
    holocron_analytics::run_until_clean(&stepped, Limits { days: 3 })
        .await
        .unwrap();
    assert_eq!(derived(&stepped).await, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_session_crossing_the_retention_cutoff_keeps_all_later_members() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    // One session of three and a half days, an event every 20 minutes.
    let start = ms("2026-06-01T00:00:00Z");
    let records: Vec<Record> = (0..=252)
        .map(|i| {
            let t = start + i * 20 * 60_000;
            site.event_at(Pageview, t, t, Who::Profile("bot"))
        })
        .collect();
    ingest(&storage, &records).await;
    correct(&storage).await;

    let cutoff = ms("2026-06-01T12:00:00Z");
    let cutoff_day = site.policies.identity_day(cutoff).local_day();
    storage.expire(SITE, cutoff, cutoff_day).await.unwrap();
    correct(&storage).await;
    let kept: Vec<&Record> = records
        .iter()
        .filter(|r| matches!(r, Record::Event(e) if e.event_time_ms >= cutoff))
        .collect();
    assert_eq!(groups(&storage, SITE).await, vec![ids(&kept)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_reads_its_events_in_order_and_its_identified_properties() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let a = site.event(Pageview, "2026-06-02T08:30:00Z", Who::Anon("v4"));
    let signup = site.event(CustomEvent, "2026-06-02T08:31:00Z", Who::Anon("v4"));
    let b = site.event(Pageview, "2026-06-02T08:40:00Z", Who::Anon("v4"));
    let other = site.event(Pageview, "2026-06-02T08:35:00Z", Who::Anon("v5"));
    let plan = identify(&site, 8, "2026-06-02T08:32:00Z", "v4");
    ingest(&storage, &[a, signup, b, other, plan]).await;
    correct(&storage).await;

    let (detail, unknown) = storage
        .read(Lane::Report, |s| {
            let id: i64 = s.connection().query_row(
                "SELECT session_id FROM sessions ORDER BY start_time LIMIT 1",
                [],
                |r| r.get(0),
            )?;
            let detail = session(s, SITE, id, &[])?.unwrap();
            Ok((detail, session(s, SITE, id + 1_000_000, &[])?))
        })
        .await
        .unwrap()
        .value;
    assert!(unknown.is_none());
    assert!(!detail.person);
    assert_eq!((detail.pageviews, detail.custom_events), (2, 1));
    assert_eq!(detail.last - detail.start, 10 * 60_000);
    let kinds: Vec<(bool, Option<&str>)> = detail
        .events
        .iter()
        .map(|e| (e.pageview, e.event_name.as_deref()))
        .collect();
    assert_eq!(kinds, [(true, None), (false, Some("signup")), (true, None)]);
    assert!(!detail.truncated);
    assert_eq!(
        detail.properties,
        [("plan".to_owned(), Cell::String("pro".into()))]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timeline_lists_the_window_s_sessions_latest_first_with_their_events() {
    let (_dir, storage) = open().await;
    let mut site = vienna();
    let before = site.event(Pageview, "2026-06-02T07:50:00Z", Who::Anon("v1"));
    let a = site.event(Pageview, "2026-06-02T08:30:00Z", Who::Anon("v2"));
    let signup = site.event(CustomEvent, "2026-06-02T08:31:00Z", Who::Anon("v2"));
    let b = site.event(Pageview, "2026-06-02T08:40:00Z", Who::Anon("v3"));
    let after = site.event(Pageview, "2026-06-02T09:00:00Z", Who::Anon("v4"));
    ingest(&storage, &[before, a, signup, b, after]).await;
    correct(&storage).await;

    let (from, to) = (ms("2026-06-02T08:00:00Z"), ms("2026-06-02T09:00:00Z"));
    let read = storage
        .read(Lane::Report, move |s| timeline(s, SITE, from, to))
        .await
        .unwrap()
        .value;
    assert_eq!(read.total, 2);
    assert_eq!(read.median_span, Some(0));
    let starts: Vec<i64> = read.sessions.iter().map(|s| s.start).collect();
    assert_eq!(
        starts,
        [ms("2026-06-02T08:40:00Z"), ms("2026-06-02T08:30:00Z")]
    );
    let events: Vec<(i64, bool, Option<&str>)> = read.sessions[1]
        .events
        .iter()
        .map(|e| (e.at, e.pageview, e.event_name.as_deref()))
        .collect();
    assert_eq!(
        events,
        [
            (ms("2026-06-02T08:30:00Z"), true, None),
            (ms("2026-06-02T08:31:00Z"), false, Some("signup")),
        ]
    );
    assert_eq!(read.sessions[1].custom_events, 1);
    assert!(!read.sessions[1].truncated);
}
