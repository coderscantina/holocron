//! Correction probe ("Handoff, corrections, and failure checks" in the
//! performance contract; manifest `correction`).
//!
//! 1. Derives every dirty day of a loaded store in bounded steps and records
//!    each step's duration: how long correction holds the analytical writer.
//! 2. Sends the manifest's 1,000 late events, 200 keyless events and 100
//!    identify changes on a fixed schedule while a follower corrects after each
//!    watermark change. Every accepted record is timed from acknowledgement to
//!    visible (watermark covers it) and to corrected (its identity day has no
//!    dirty row from its batch or earlier).
//! 3. Checks the sessions of every anonymous late-event target against the
//!    0002 timeout rule computed here from the generator's events.
//!
//! Late-event targets are the loaded September 2026 groups whose start lies in
//! 01:30 to 22:30 UTC of their day, instead of 2026-09-30 only: a store loaded
//! with fewer groups per month holds each month's first days. Late events keep
//! the target's pseudonym, as the manifest's transport does. Revenue amounts of
//! the purchase events are left out; revenue deltas belong to milestone 08.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use holocron_analytics::Limits;
use holocron_storage::{
    EventKind, Identify, Lane, Outcome, Property, PropertyValue, Record, Storage,
};
use serde_json::{Value as Json, json};
use tokio::sync::Mutex;

use crate::bench::{memory, stats, write_lines};
use crate::catalog::{Catalog, fixture_uuid};
use crate::fixture::{Fixture, Group, Kind, local_date};
use crate::load::{Identities, days_since_epoch, records};
use crate::prng::{Stream, uniform};

const SEPTEMBER_2026: u64 = 11;
const WINDOW_MS: i64 = 30 * 60_000;

#[derive(clap::Args)]
pub struct Args {
    #[arg(long)]
    pub data: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    /// Groups per month the store was loaded with.
    #[arg(long)]
    pub groups_per_month: u64,
    /// Spacing of late events; the manifest's schedule is 1,800 ms.
    #[arg(long, default_value_t = 1_800)]
    pub late_spacing_ms: u64,
    /// Identity days rebuilt per correction step.
    #[arg(long, default_value_t = Limits::default().days)]
    pub step_days: usize,
}

/// One sent record waiting to become visible and corrected.
struct Pending {
    stream: &'static str,
    site: u128,
    local_day: i32,
    acked: Instant,
    visible: Option<Instant>,
}

pub async fn run(fixtures: &Path, args: Args) {
    std::fs::create_dir_all(&args.out).unwrap();
    let cat: &'static Catalog = Catalog::load(&fixtures.join("workload/manifest.json"));
    let fx = Fixture::new(cat);
    let ids = Identities::new(cat);
    let storage = Arc::new(
        Storage::open(&args.data, crate::gate::budgets())
            .await
            .expect("open storage"),
    );
    let limits = Limits {
        days: args.step_days,
    };
    let started = Instant::now();
    let mut resources = vec![resource_sample(&args.data, 0.0, "start")];

    // 1. Initial derivation in bounded steps.
    let mut backfill_steps = Vec::new();
    loop {
        let t = Instant::now();
        let step = holocron_analytics::step(&storage, limits)
            .await
            .expect("backfill step");
        backfill_steps.push(json!({
            "ms": t.elapsed().as_secs_f64() * 1e3, "days": step.days, "profiles": step.profiles,
            "sessions": step.sessions, "remaining": step.remaining,
        }));
        if step.remaining == 0 {
            break;
        }
    }
    let backfill_s = started.elapsed().as_secs_f64();
    resources.push(resource_sample(&args.data, backfill_s, "after backfill"));
    eprintln!(
        "correction: backfill {} steps in {backfill_s:.1}s",
        backfill_steps.len()
    );

    // 2. Late records while a follower corrects.
    let targets = late_targets(&fx, args.groups_per_month);
    assert!(
        !targets.is_empty(),
        "no loaded September 2026 group starts between 01:30 and 22:30 UTC"
    );
    let plan = schedule(cat, &fx, &ids, &targets, args.late_spacing_ms);
    let pending: Arc<Mutex<HashMap<i64, Pending>>> = Arc::default();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let follower = tokio::spawn(follow(storage.clone(), limits, stopped.clone()));
    let probe = tokio::spawn(probe(storage.clone(), pending.clone(), stopped));
    let origin = tokio::time::Instant::now();
    let send_started = started.elapsed().as_secs_f64();
    let mut sends = tokio::task::JoinSet::new();
    for (index, (offset_ms, stream, record)) in plan.iter().cloned().enumerate() {
        tokio::time::sleep_until(origin + Duration::from_millis(offset_ms)).await;
        let (storage, pending) = (storage.clone(), pending.clone());
        sends.spawn(async move {
            let Ok(outcome) = storage.admit(std::slice::from_ref(&record)).await else {
                return None;
            };
            if let Outcome::Accepted { sequence } = outcome[0] {
                let (site, local_day) = match &record {
                    Record::Event(e) => (e.site, e.local_day),
                    Record::Identify(i) => (i.site, i.local_day),
                    _ => unreachable!("the plan holds events and identify messages"),
                };
                pending.lock().await.insert(
                    sequence,
                    Pending {
                        stream,
                        site,
                        local_day,
                        acked: Instant::now(),
                        visible: None,
                    },
                );
            }
            Some(index)
        });
    }
    let mut accepted = vec![false; plan.len()];
    while let Some(sent) = sends.join_next().await {
        if let Some(index) = sent.unwrap() {
            accepted[index] = true;
        }
    }
    let refused = accepted.iter().filter(|a| !**a).count();
    resources.push(resource_sample(
        &args.data,
        started.elapsed().as_secs_f64(),
        "all sent",
    ));
    let deadline = Instant::now() + Duration::from_secs(600);
    while !pending.lock().await.is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = stop.send(true);
    let correction_steps = follower.await.unwrap();
    let samples = probe.await.unwrap();
    let unfinished = pending.lock().await.len();
    resources.push(resource_sample(
        &args.data,
        started.elapsed().as_secs_f64(),
        "corrected",
    ));

    // 3. Sessions of anonymous late-event targets against the timeout rule.
    let delivered: Vec<_> = plan
        .iter()
        .zip(&accepted)
        .filter(|(_, a)| **a)
        .map(|(p, _)| p.clone())
        .collect();
    let oracle = check_sessions(&storage, cat, &ids, &targets, &delivered).await;

    let by_stream = |field: &str| {
        let mut out = BTreeMap::new();
        for stream in ["late", "keyless", "identify"] {
            let values = samples
                .iter()
                .filter(|s| s["stream"] == stream)
                .filter_map(|s| s[field].as_f64())
                .collect();
            out.insert(stream, stats(values));
        }
        out
    };
    let summary = json!({
        "groups_per_month": args.groups_per_month,
        "step_days": args.step_days,
        "late_spacing_ms": args.late_spacing_ms,
        "late_targets": targets.len(),
        "backfill": {
            "seconds": backfill_s,
            "steps": backfill_steps.len(),
            "step_ms": stats(backfill_steps.iter().map(|s| s["ms"].as_f64().unwrap()).collect()),
            "sessions": backfill_steps.iter().map(|s| s["sessions"].as_u64().unwrap()).sum::<u64>(),
        },
        "sent": plan.len(),
        "refused": refused,
        "send_window_s": [send_started, send_started + plan.last().map_or(0, |p| p.0) as f64 / 1e3],
        "unfinished": unfinished,
        "ack_to_visible_ms": by_stream("ack_to_visible_ms"),
        "ack_to_corrected_ms": by_stream("ack_to_corrected_ms"),
        "correction_steps": correction_steps.len(),
        "correction_step_ms": stats(correction_steps.iter().map(|s| s["ms"].as_f64().unwrap()).collect()),
        "session_oracle": oracle,
        "resources": resources,
    });
    write_lines(&args.out.join("backfill-steps.jsonl"), &backfill_steps);
    write_lines(&args.out.join("correction-steps.jsonl"), &correction_steps);
    write_lines(&args.out.join("corrected.jsonl"), &samples);
    std::fs::write(
        args.out.join("correction-summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    drop(
        Arc::into_inner(storage)
            .expect("storage still shared")
            .close()
            .await,
    );
}

fn resource_sample(data: &Path, at_s: f64, label: &str) -> Json {
    // macOS has no cgroup files; `ps` gives the resident set there.
    let rss = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok());
    json!({
        "at_s": at_s, "label": label, "rss_kib": rss, "linux": memory(),
        "analytics_bytes": std::fs::metadata(data.join("analytics.duckdb")).map(|m| m.len()).ok(),
        "spill_bytes": crate::bench::dir_size(&data.join("spill")),
    })
}

/// Loaded September 2026 groups that start between 01:30 and 22:30 UTC, by ordinal.
fn late_targets(fx: &Fixture, groups_per_month: u64) -> Vec<Group> {
    let mut targets: Vec<Group> = fx
        .month(SEPTEMBER_2026, groups_per_month)
        .filter(|g| (5_400..=81_000).contains(&g.t0_s.rem_euclid(86_400)))
        .collect();
    targets.sort_by_key(|g| g.g);
    targets
}

fn last_eligible_ms(g: &Group) -> i64 {
    g.events
        .iter()
        .filter(|e| e.kind != Kind::Performance)
        .map(|e| e.time_s)
        .max()
        .unwrap()
        * 1000
}

/// A group's canonical event with a new ID, time and kind, re-keyed when its
/// local day differs from the group's.
fn late_record(
    cat: &Catalog,
    ids: &Identities,
    g: &Group,
    id: &str,
    time_ms: i64,
    kind: EventKind,
) -> Record {
    let Some(Record::Event(mut e)) = records(cat, ids, g)
        .into_iter()
        .find(|r| matches!(r, Record::Event(_)))
    else {
        unreachable!("every group has events")
    };
    let site = &cat.sites[g.site];
    let day = local_date(site.timezone, time_ms.div_euclid(1000));
    if day != g.local_day {
        e.daily_id = Some(match &g.supplied_id {
            Some(supplied) => ids.daily_identified(g.site, day, supplied),
            None => ids.anonymous(g.site, day, &g.ip.to_string(), &g.user_agent),
        });
    }
    e.id = fixture_uuid(id).as_u128();
    e.kind = kind;
    e.event_time_ms = time_ms;
    e.receive_time_ms = time_ms;
    e.local_day = days_since_epoch(day);
    e.performance = None;
    e.revenue = None;
    e.properties = Vec::new();
    e.dimensions.event_name = None;
    Record::Event(e)
}

/// `(send offset ms, stream, record)` in send order.
fn schedule(
    cat: &Catalog,
    fx: &Fixture,
    ids: &Identities,
    targets: &[Group],
    spacing_ms: u64,
) -> Vec<(u64, &'static str, Record)> {
    let value = Stream::named("correction.value");
    let n = targets.len() as u64;
    let mut plan = Vec::new();
    for k in 0..1000u64 {
        let g = &targets[(k * n / 1000) as usize];
        let first = g.events.iter().map(|e| e.time_s).min().unwrap() * 1000;
        let last = last_eligible_ms(g);
        let draw = |range: u64| uniform(value.at(k, 1), range) as i64 * 1000;
        let id = format!("late:{k}");
        let mut record = match k {
            0..=399 => late_record(
                cat,
                ids,
                g,
                &id,
                last + 60_000 + draw(1201),
                EventKind::Pageview,
            ),
            400..=599 => late_record(
                cat,
                ids,
                g,
                &id,
                first - 60_000 - draw(1201),
                EventKind::Pageview,
            ),
            600..=799 => late_record(
                cat,
                ids,
                g,
                &id,
                last + 30_000 + draw(571),
                EventKind::CustomEvent,
            ),
            800..=899 => late_record(cat, ids, g, &id, last + 30_000, EventKind::CustomEvent),
            _ => late_record(cat, ids, g, &id, last + 10_000, EventKind::Performance),
        };
        if let Record::Event(e) = &mut record {
            match k {
                400..=599 => {
                    e.dimensions.referrer_domain = Some("bing.com".into());
                    e.dimensions.utm_campaign = Some("cmp-late".into());
                }
                600..=799 => {
                    e.dimensions.event_name = Some("signup".into());
                    e.properties = vec![Property {
                        key: "plan".into(),
                        value: PropertyValue::String("pro".into()),
                    }];
                }
                800..=899 => e.dimensions.event_name = Some("purchase".into()),
                _ => {}
            }
        }
        plan.push((k * spacing_ms, "late", record));
    }
    // Keyless: month i = k mod 11, anonymous target i * 10^6 + 500000 + k, identified i * 10^6 + k.
    for k in 0..200u64 {
        let month = k % 11;
        let target = if k < 150 {
            month * 1_000_000 + 500_000 + k
        } else {
            month * 1_000_000 + k
        };
        let g = fx.group(target);
        let kind = if (100..150).contains(&k) {
            EventKind::CustomEvent
        } else {
            EventKind::Pageview
        };
        let mut record = late_record(
            cat,
            ids,
            &g,
            &format!("keyless:{k}"),
            last_eligible_ms(&g) + 60_000,
            kind,
        );
        if let Record::Event(e) = &mut record {
            e.daily_id = None;
            if k < 150 {
                e.profile_id = None;
            }
            if kind == EventKind::CustomEvent {
                e.dimensions.event_name = Some("purchase".into());
            }
        }
        plan.push((k * spacing_ms * 5, "keyless", record));
    }
    // Identify changes: even j 61 s before the group's start, odd j 120 s after.
    let identified: Vec<&Group> = targets.iter().filter(|g| g.identify.is_some()).collect();
    for j in 0..100u64.min(identified.len() as u64 * 100) {
        let g = identified[(j * identified.len() as u64 / 100) as usize];
        let Some(Record::Identify(mut i)) = records(cat, ids, g).into_iter().next() else {
            unreachable!("identified groups start with identify")
        };
        let time = if j % 2 == 0 {
            g.t0_s * 1000 - 61_000
        } else {
            g.t0_s * 1000 + 120_000
        };
        i = Identify {
            id: fixture_uuid(&format!("identify-change:{j}")).as_u128(),
            event_time_ms: time,
            receive_time_ms: time,
            properties: vec![Property {
                key: "account_tier".into(),
                value: PropertyValue::String("changed".into()),
            }],
            ..i
        };
        plan.push((j * spacing_ms * 10, "identify", Record::Identify(i)));
    }
    plan.sort_by_key(|p| p.0);
    plan
}

/// Runs correction steps after every watermark change; returns step timings.
async fn follow(
    storage: Arc<Storage>,
    limits: Limits,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Vec<Json> {
    let mut watermark = storage.watermark();
    let mut steps = Vec::new();
    loop {
        loop {
            let t = Instant::now();
            let step = holocron_analytics::step(&storage, limits)
                .await
                .expect("correction step");
            if step.days > 0 {
                steps.push(json!({
                    "ms": t.elapsed().as_secs_f64() * 1e3, "days": step.days, "profiles": step.profiles,
                    "sessions": step.sessions, "remaining": step.remaining,
                }));
            }
            if step.remaining == 0 {
                break;
            }
        }
        if *stop.borrow() {
            return steps;
        }
        tokio::select! {
            _ = watermark.changed() => {}
            _ = stop.changed() => {}
        }
    }
}

/// Every 100 ms, one snapshot: watermark, batch ranges and dirty days.
async fn probe(
    storage: Arc<Storage>,
    pending: Arc<Mutex<HashMap<i64, Pending>>>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> Vec<Json> {
    let mut done = Vec::new();
    while !*stop.borrow() {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let read = storage
            .read(Lane::Live, |s| {
                let conn = s.connection();
                let mut st = conn.prepare("SELECT site, local_day, min(batch_id) FROM dirty_days GROUP BY ALL")?;
                let dirty: HashMap<(u128, i32), i64> = st
                    .query_map([], |r| {
                        let day: holocron_storage::duckdb::types::Value = r.get(1)?;
                        let holocron_storage::duckdb::types::Value::Date32(day) = day else { unreachable!() };
                        Ok(((r.get(0)?, day), r.get(2)?))
                    })?
                    .collect::<Result<_, _>>()?;
                let mut st = conn.prepare(
                    "SELECT batch_id, last_seq FROM applied_batches ORDER BY batch_id DESC LIMIT 2000",
                )?;
                let batches: Vec<(i64, i64)> =
                    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
                Ok((dirty, batches))
            })
            .await;
        let Ok(read) = read else { continue };
        let seen = Instant::now();
        let (dirty, mut batches) = read.value;
        batches.sort_unstable();
        let mut pending = pending.lock().await;
        let mut finished = Vec::new();
        for (seq, p) in pending.iter_mut() {
            if read.watermark.sequence < *seq {
                continue;
            }
            p.visible.get_or_insert(seen);
            let batch = batches
                .iter()
                .find(|(_, last)| last >= seq)
                .map(|(b, _)| *b)
                .unwrap_or(i64::MAX);
            let corrected = dirty
                .get(&(p.site, p.local_day))
                .is_none_or(|min| *min > batch);
            if corrected {
                finished.push(*seq);
                done.push(json!({
                    "stream": p.stream, "seq": seq,
                    "ack_to_visible_ms": p.visible.unwrap().duration_since(p.acked).as_secs_f64() * 1e3,
                    "ack_to_corrected_ms": seen.duration_since(p.acked).as_secs_f64() * 1e3,
                }));
            }
        }
        for seq in finished {
            pending.remove(&seq);
        }
    }
    done
}

/// Sessions of the anonymous late-event targets, by the 0002 timeout rule over
/// the group's eligible events plus its late events, per daily pseudonym.
async fn check_sessions(
    storage: &Storage,
    cat: &Catalog,
    ids: &Identities,
    targets: &[Group],
    plan: &[(u64, &'static str, Record)],
) -> Json {
    let mut expected: BTreeMap<(u128, u128), Vec<(i64, bool)>> = BTreeMap::new();
    for g in targets.iter().filter(|g| g.supplied_id.is_none()) {
        for record in records(cat, ids, g) {
            if let Record::Event(e) = record
                && e.kind != EventKind::Performance
            {
                expected
                    .entry((e.site, e.daily_id.unwrap()))
                    .or_default()
                    .push((e.event_time_ms, e.kind == EventKind::Pageview));
            }
        }
    }
    for (_, stream, record) in plan {
        if let (&"late", Record::Event(e)) = (stream, record)
            && e.kind != EventKind::Performance
            && e.profile_id.is_none()
            && let Some(list) = expected.get_mut(&(e.site, e.daily_id.unwrap()))
        {
            list.push((e.event_time_ms, e.kind == EventKind::Pageview));
        }
    }
    // Expected sessions per (site, daily id): (start, end, pageviews).
    type Sessions = Vec<(i64, i64, i64)>;
    let mut want: BTreeMap<(u128, u128), Sessions> = BTreeMap::new();
    for (key, mut events) in expected {
        events.sort();
        let sessions = want.entry(key).or_default();
        let mut last = i64::MIN;
        for (t, pageview) in events {
            if sessions.is_empty() || t - last >= WINDOW_MS {
                sessions.push((t, t, 0));
            }
            let current = sessions.last_mut().unwrap();
            current.1 = t;
            current.2 += i64::from(pageview);
            last = t;
        }
    }
    let identities: Vec<(u128, u128)> = want.keys().copied().collect();
    let got = storage
        .read(Lane::Report, move |s| {
            let mut st = s.connection().prepare(
                "SELECT epoch_ms(start_time), epoch_ms(last_time), pageviews FROM sessions
                 WHERE site = ? AND population = 1 AND identity = ? ORDER BY start_time",
            )?;
            let mut out = BTreeMap::new();
            for (site, identity) in identities {
                let rows: Vec<(i64, i64, i64)> = st
                    .query_map(holocron_storage::duckdb::params![site, identity], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?
                    .collect::<Result<_, _>>()?;
                out.insert((site, identity), rows);
            }
            Ok(out)
        })
        .await
        .expect("read target sessions")
        .value;
    let mismatched: Vec<String> = want
        .iter()
        .filter(|(key, sessions)| got.get(key) != Some(sessions))
        .map(|(key, _)| format!("{:032x}", key.1))
        .collect();
    json!({
        "identities": want.len(),
        "sessions": want.values().map(Vec::len).sum::<usize>(),
        "mismatched": mismatched.len(),
        "mismatched_identities": mismatched.into_iter().take(10).collect::<Vec<_>>(),
    })
}
