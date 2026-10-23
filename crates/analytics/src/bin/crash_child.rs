//! Test support for `tests/crash.rs`, built only with the `failpoints` feature.
//!
//! `run <dir> <seed>` admits three quarters of the seeded records, derives
//! everything in one correction write, saves that state, then admits the rest
//! (late events for days already corrected) and corrects in small steps. The
//! parent arms `write.before_commit`: hit 1 kills the first derivation, later
//! hits kill a step that replaces existing rows.
//!
//! `verify <dir> <seed>` reopens the store without failpoints and checks that
//! the interrupted correction left, per identity day and per profile session,
//! either the saved state or the complete result: a reference built from all
//! records in a fresh store. Days that differ from the reference must still be dirty,
//! so freshness labels them pending. It then finishes correction and requires
//! the reference exactly. Prints `ok <summary>` or exits non-zero.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use holocron_analytics::{Limits, run_until_clean, step};
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Identify, Lane, Property, PropertyValue, Record, Storage,
};

const DAY_MS: i64 = 86_400_000;
const START_MS: i64 = 1_780_272_000_000; // 2026-06-01T00:00:00Z
const STEP: Limits = Limits { days: 3 };

fn budgets() -> Budgets {
    Budgets {
        transfer_delay: Duration::from_millis(2),
        batch_records: 17,
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        (((z ^ (z >> 31)) as u128 * u128::from(n)) >> 64) as u64
    }
}

/// Eight UTC days on two sites; anonymous visitors and consented profiles whose
/// sessions cross midnight; events arrive out of event-time order.
fn records(seed: u64) -> Vec<Record> {
    let mut rng = Rng(seed ^ 0x484F4C4F);
    let sites = [1u128 << 100, 2u128 << 100];
    let mut out = Vec::new();
    for n in 0..240u128 {
        let site = sites[rng.below(2) as usize];
        let day = rng.below(8) as i64;
        // Near midnight half the time, so profile sessions cross it.
        let offset = if rng.below(2) == 0 {
            DAY_MS - 3_600_000 + rng.below(7_200_000) as i64
        } else {
            rng.below(DAY_MS as u64) as i64
        };
        let time = START_MS + day * DAY_MS + offset;
        let local_day = (time / DAY_MS) as i32;
        let visitor = rng.below(6) as u128;
        let profile = (rng.below(3) == 0).then_some(900 + visitor % 3);
        let daily_id = Some((u128::from(local_day as u32) << 64) | (site >> 96) << 32 | visitor);
        let id = (u128::from(seed) << 64) | n;
        out.push(match rng.below(10) {
            0 => Record::Identify(Identify {
                id,
                site,
                event_time_ms: time,
                receive_time_ms: time,
                local_day,
                daily_id,
                profile_id: profile,
                properties: vec![Property {
                    key: "tier".into(),
                    value: PropertyValue::Integer(n as i64),
                }],
            }),
            k => Record::Event(Event {
                id,
                site,
                kind: match k {
                    1 => EventKind::Performance,
                    2..=4 => EventKind::CustomEvent,
                    _ => EventKind::Pageview,
                },
                event_time_ms: time,
                receive_time_ms: time,
                local_day,
                daily_id,
                profile_id: profile,
                client_event_id: None,
                dimensions: Dimensions::default(),
                performance: None,
                revenue: None,
                properties: Vec::new(),
            }),
        });
    }
    out
}

const FIRST_WAVE: usize = 180;

async fn admit(storage: &Storage, records: &[Record]) {
    for chunk in records.chunks(31) {
        storage.admit(chunk).await.expect("admit");
    }
    storage.drain().await;
}

/// Saved derived state, one `kind<TAB>key<TAB>row` line per row.
fn save(derived: &Derived, path: &Path) {
    let mut out = String::new();
    for (kind, map) in [("day", &derived.by_day), ("profile", &derived.by_profile)] {
        for (key, rows) in map {
            for row in rows {
                out += &format!("{kind}\t{key}\t{row}\n");
            }
        }
    }
    std::fs::write(path, out).expect("save state");
}

fn load(path: &Path) -> Derived {
    let mut derived = Derived::default();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let mut parts = line.splitn(3, '\t');
        let (kind, key, row) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let map = if kind == "day" {
            &mut derived.by_day
        } else {
            &mut derived.by_profile
        };
        map.entry(key.to_owned())
            .or_default()
            .insert(row.to_owned());
    }
    derived
}

/// Derived rows keyed for comparison: daily sessions and aggregates by day, profile sessions by profile.
#[derive(Default, PartialEq, Eq, Debug)]
struct Derived {
    by_day: BTreeMap<String, BTreeSet<String>>,
    by_profile: BTreeMap<String, BTreeSet<String>>,
    dirty: BTreeSet<String>,
}

async fn derived(storage: &Storage) -> Derived {
    storage
        .read(Lane::Report, |s| {
            let conn = s.connection();
            let mut out = Derived::default();
            let mut st = conn.prepare(
                "SELECT s.site || ' ' || s.local_day, s.population, s.site || ' ' || s.identity,
                        s.session_id || ' ' || s.start_time || ' ' || s.last_time || ' ' || s.ends_at || ' ' ||
                        s.pageviews || ' ' || s.custom_events || ' ' || s.policy_version || ' ' ||
                        (SELECT string_agg(m.seq || ':' || m.role, ',' ORDER BY m.seq) FROM session_members m
                         WHERE m.site = s.site AND m.session_id = s.session_id)
                 FROM sessions s",
            )?;
            for row in st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u8>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            })? {
                let (day, population, identity, session) = row?;
                match population {
                    1 => out.by_day.entry(day).or_default().insert(session),
                    _ => out.by_profile.entry(identity).or_default().insert(session),
                };
            }
            let mut st = conn.prepare(
                "SELECT site || ' ' || local_day, session_policy_versions::VARCHAR || pageviews || ' ' || custom_events || ' ' ||
                        eligible_events || ' ' || visitors || ' ' || sessions || ' ' || bounces || ' ' || span_sum_ms
                 FROM day_aggregates",
            )?;
            for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (day, aggregate) = row?;
                out.by_day.entry(day).or_default().insert(format!("day {aggregate}"));
            }
            let mut st = conn.prepare("SELECT DISTINCT site || ' ' || local_day FROM dirty_days")?;
            for day in st.query_map([], |r| r.get::<_, String>(0))? {
                out.dirty.insert(day?);
            }
            Ok(out)
        })
        .await
        .expect("read derived")
        .value
}

/// A dirty day may differ from its old rows only in its aggregate row, which a
/// profile rebuild from another day refreshes; its daily sessions stay old.
fn profile_refresh_only(got: Option<&BTreeSet<String>>, before: Option<&BTreeSet<String>>) -> bool {
    let sessions = |rows: Option<&BTreeSet<String>>| {
        rows.into_iter()
            .flatten()
            .filter(|r| !r.starts_with("day "))
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    sessions(got) == sessions(before)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[2]);
    let seed: u64 = args[3].parse().expect("seed");
    match args[1].as_str() {
        "run" => {
            let storage = Storage::open(dir, budgets()).await.expect("open");
            let records = records(seed);
            admit(&storage, &records[..FIRST_WAVE]).await;
            // One write derives every dirty day, so hit 1 is the first derivation.
            step(
                &storage,
                Limits {
                    days: usize::MAX >> 1,
                },
            )
            .await
            .expect("first derivation");
            save(&derived(&storage).await, &dir.join("before.state"));
            admit(&storage, &records[FIRST_WAVE..]).await;
            loop {
                if step(&storage, STEP).await.expect("step").remaining == 0 {
                    break;
                }
            }
            storage.close().await.expect("close");
        }
        "verify" => {
            let storage = Storage::open(dir, budgets()).await.expect("reopen");
            storage.drain().await;
            let interrupted = derived(&storage).await;

            let before = load(&dir.join("before.state"));
            let reference_dir = tempfile::tempdir().expect("tempdir");
            let reference_store = Storage::open(reference_dir.path(), budgets())
                .await
                .expect("open reference");
            admit(&reference_store, &records(seed)).await;
            run_until_clean(&reference_store, Limits::default())
                .await
                .expect("reference");
            let reference = derived(&reference_store).await;

            let mut violations = Vec::new();
            let keys = |a: &BTreeMap<String, BTreeSet<String>>,
                        b: &BTreeMap<String, BTreeSet<String>>,
                        c: &BTreeMap<String, BTreeSet<String>>| {
                a.keys()
                    .chain(b.keys())
                    .chain(c.keys())
                    .cloned()
                    .collect::<BTreeSet<String>>()
            };
            let mut replaced = 0;
            for day in keys(&interrupted.by_day, &before.by_day, &reference.by_day) {
                let got = interrupted.by_day.get(&day);
                if got == reference.by_day.get(&day) {
                    replaced += usize::from(got != before.by_day.get(&day));
                } else if !interrupted.dirty.contains(&day) {
                    violations.push(format!("day {day} is clean but differs from the reference"));
                } else if got != before.by_day.get(&day)
                    && !profile_refresh_only(got, before.by_day.get(&day))
                {
                    violations.push(format!(
                        "dirty day {day} holds neither the old nor the new sessions"
                    ));
                }
            }
            // Correction rebuilds a span of a profile's history, so a profile may hold old
            // and new sessions side by side, but each session is one or the other.
            for (profile, sessions) in &interrupted.by_profile {
                let known = |state: &Derived, session: &String| {
                    state
                        .by_profile
                        .get(profile)
                        .is_some_and(|rows| rows.contains(session))
                };
                for session in sessions {
                    if !known(&before, session) && !known(&reference, session) {
                        violations.push(format!(
                            "profile {profile} holds session {session}, neither old nor new"
                        ));
                    }
                }
            }
            // Killed in the first derivation: the late wave was never sent.
            if !dir.join("before.state").exists() {
                admit(&storage, &records(seed)[FIRST_WAVE..]).await;
            }
            run_until_clean(&storage, Limits::default())
                .await
                .expect("finish");
            let finished = derived(&storage).await;
            if finished != reference {
                violations.push("finished correction differs from the reference".into());
            }
            if !violations.is_empty() {
                eprintln!("{}", violations.join("\n"));
                std::process::exit(1);
            }
            println!(
                "ok: {replaced} days replaced before the kill, {} still dirty; finished state equals the reference",
                interrupted.dirty.len()
            );
            reference_store.close().await.expect("close reference");
            storage.close().await.expect("close");
        }
        other => panic!("unknown command {other}"),
    }
}
