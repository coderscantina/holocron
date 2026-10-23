//! Adds consented returning profiles, purchases with refunds and web vitals to the
//! "Roadhouse Konzertagentur" site of `seed_demo` in a stopped, seeded data directory, for
//! browser checks of retention, revenue and performance. Development only; run it after
//! `seed_traffic`.
//!
//! `cargo run -p holocron-server --example seed_commerce -- <data-dir> [days] [--now <instant>]`
//!
//! It turns persistent identity on for the site, as the identity settings would, and admits
//! records through admission, transfer and correction like collected ones. Profile and
//! pseudonym IDs are stand-in hashes.

use std::path::PathBuf;

use holocron_analytics::{Limits, run_until_clean};
use holocron_server::db::Db;
use holocron_server::sites::policy::{analytics_id, session_policies};
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Performance, Property, PropertyValue, Record, Revenue,
    Storage,
};

const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// SplitMix64, so every run seeds the same records for the same start instant.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn id(&mut self) -> u128 {
        u128::from(self.next()) << 64 | u128::from(self.next())
    }
}

const PATHS: [&str; 4] = ["/de/events", "/de", "/de/checkout", "/de/checkout/payment"];
/// Browser, OS, device.
const CLIENTS: [(&str, &str, &str); 3] = [
    ("chrome", "Android OS", "mobile"),
    ("ios", "iOS", "mobile"),
    ("chrome", "Windows 10", "desktop"),
];

#[path = "support/now.rs"]
mod now;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (now, args) = now::from_args()?;
    let mut args = args.into_iter();
    let Some(dir) = args.next() else {
        anyhow::bail!("usage: seed_commerce <data-dir> [days] [--now <instant>]");
    };
    let days: i64 = args.next().map(|d| d.parse()).transpose()?.unwrap_or(45);
    let dir = PathBuf::from(dir);
    let db = Db::open(&dir).await?;
    // By name, as `e2e/retention-revenue.spec.ts` expects: the demo sites share one creation
    // time, so "the first site" would depend on their random IDs.
    let (site, domain): (String, String) = sqlx::query_as(
        "SELECT id, domain FROM sites WHERE name LIKE 'Roadhouse Konzertagentur%' ORDER BY id LIMIT 1",
    )
    .fetch_optional(&db.reader)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no Roadhouse Konzertagentur site: run seed_demo first"))?;
    sqlx::query(
        "UPDATE sites SET persistent_identity = 1, identity_policy_version = identity_policy_version + 1
         WHERE id = ? AND persistent_identity = 0",
    )
    .bind(&site)
    .execute(&db.writer)
    .await?;
    let storage = Storage::open(
        &dir,
        Budgets {
            disk_reserve_bytes: 256 * 1024 * 1024,
            ..Budgets::default()
        },
    )
    .await?;
    let policies = session_policies(&db.reader, &site)
        .await?
        .ok_or_else(|| anyhow::anyhow!("site {site} has no session policy"))?;
    let site_id = analytics_id(&site)?;

    let mut rng = Rng(now as u64 ^ 0x5eed);
    let mut records = Vec::new();
    let event = |rng: &mut Rng,
                 kind: EventKind,
                 at: i64,
                 profile: Option<u128>,
                 daily: u128,
                 client: (&str, &str, &str),
                 path: &str| {
        let (browser, os, device) = client;
        Event {
            id: rng.id(),
            site: site_id,
            kind,
            event_time_ms: at,
            receive_time_ms: at.max(now - DAY_MS),
            local_day: policies.identity_day(at).local_day(),
            daily_id: Some(daily),
            profile_id: profile,
            client_event_id: None,
            dimensions: Dimensions {
                hostname: Some(domain.clone()),
                path: Some(path.into()),
                country: Some(if rng.below(4) == 0 { "DE" } else { "AT" }.into()),
                browser: Some(browser.into()),
                os: Some(os.into()),
                device: Some(device.into()),
                language: Some("de-AT".into()),
                ..Dimensions::default()
            },
            performance: None,
            revenue: None,
            properties: Vec::new(),
        }
    };

    // Consented profiles: a first visit somewhere in range, then return visits whose chance
    // halves every two weeks.
    for _ in 0..140 {
        let profile = rng.id();
        let client = CLIENTS[rng.below(3) as usize];
        let first = now - rng.below((days * DAY_MS) as u64) as i64;
        let mut day = 0;
        while first + day * DAY_MS < now {
            let returning = day == 0 || rng.below(100) < 45 * 14 / (14 + day as u64) / 2;
            if returning {
                let at = (first + day * DAY_MS + rng.below(3_600_000) as i64).min(now);
                let daily = rng.id();
                for page in 0..1 + rng.below(3) as i64 {
                    let path = PATHS[rng.below(2) as usize];
                    let at = (at + page * 2 * MINUTE_MS).min(now);
                    records.push(event(
                        &mut rng,
                        EventKind::Pageview,
                        at,
                        Some(profile),
                        daily,
                        client,
                        path,
                    ));
                }
            }
            day += 1;
        }
    }

    // Purchases in EUR and USD, every tenth one refunded the next day, and one JPY order.
    for n in 0..220 {
        let at = now - rng.below((days * DAY_MS) as u64) as i64;
        let client = CLIENTS[rng.below(3) as usize];
        let daily = rng.id();
        let (currency, amount) = match (n % 7, n) {
            (_, 0) => ("JPY", 4500),
            (0 | 1, _) => ("USD", 1500 + rng.below(9000) as i64),
            _ => ("EUR", 990 + rng.below(12_000) as i64),
        };
        let mut purchase = |rng: &mut Rng, at: i64, minor_units: i64| {
            let mut e = event(
                rng,
                EventKind::CustomEvent,
                at,
                None,
                daily,
                client,
                "/de/checkout/payment",
            );
            e.dimensions.event_name = Some("purchase".into());
            e.revenue = Some(Revenue {
                currency: currency.into(),
                minor_units,
            });
            e.properties = vec![Property {
                key: "currency".into(),
                value: PropertyValue::String(currency.into()),
            }];
            records.push(e);
        };
        purchase(&mut rng, at, amount);
        if n % 10 == 3 && at + DAY_MS < now {
            purchase(&mut rng, at + DAY_MS, -amount);
        }
    }

    // Web vitals: LCP, FCP and TTFB on every report, INP on most, CLS on most; mobile is slower.
    for _ in 0..900 {
        let at = now - rng.below((days * DAY_MS) as u64) as i64;
        let client = CLIENTS[rng.below(3) as usize];
        let slow: i32 = if client.2 == "mobile" { 2 } else { 1 };
        let daily = rng.id();
        let path = PATHS[rng.below(4) as usize];
        let mut report = event(
            &mut rng,
            EventKind::Performance,
            at,
            None,
            daily,
            client,
            path,
        );
        let ms = |rng: &mut Rng, base: i32, spread: u64| base * slow + rng.below(spread) as i32;
        report.performance = Some(Performance {
            lcp: Some(ms(&mut rng, 900, 2600)),
            fcp: Some(ms(&mut rng, 500, 1400)),
            ttfb: Some(ms(&mut rng, 120, 700)),
            inp: (rng.below(5) != 0).then(|| ms(&mut rng, 60, 300)),
            cls: (rng.below(6) != 0).then(|| rng.below(3000) as i32),
        });
        records.push(report);
    }

    let total = records.len();
    let records: Vec<Record> = records.into_iter().map(Record::Event).collect();
    for chunk in records.chunks(5_000) {
        storage
            .admit(chunk)
            .await
            .map_err(|refusal| anyhow::anyhow!("admission refused: {refusal}"))?;
    }
    storage.drain().await;
    let step = run_until_clean(&storage, Limits::default()).await?;
    storage.close().await?;
    db.close().await;
    println!(
        "seeded {total} profile, purchase and performance events over {days} days into {site}; derived {} sessions",
        step.sessions
    );
    Ok(())
}
