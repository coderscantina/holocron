//! Adds deterministic traffic to every site of a stopped, seeded data directory, for browser
//! checks of the dashboard. Development only; the packaged binary has no equivalent.
//!
//! `cargo run -p holocron-server --example seed_traffic -- <data-dir> [days] [--now <instant>]`
//!
//! Records go through admission, transfer and correction like collected ones, with identity
//! days from each site's session policy. Pseudonyms are stand-in hashes, so the browser sees
//! real derived sessions without the collection path's key rotation (events older than a day
//! sent over HTTP would arrive keyless). The last visitors land in the last 30 minutes before
//! now, for realtime: the run, or `--now` for a server on the same `--frozen-clock`.

use std::path::PathBuf;

use holocron_analytics::{Limits, run_until_clean};
use holocron_server::db::Db;
use holocron_server::sites::policy::{analytics_id, session_policies};
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Property, PropertyValue, Record, Storage,
};

const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// SplitMix64, so every run seeds the same traffic for the same start instant.
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

    /// One of `items`, weighted by the second element.
    fn pick<'a, T>(&mut self, items: &'a [(T, u64)]) -> &'a T {
        let total: u64 = items.iter().map(|(_, w)| w).sum();
        let mut roll = self.below(total);
        for (item, weight) in items {
            if roll < *weight {
                return item;
            }
            roll -= weight;
        }
        &items[0].0
    }
}

const PATHS: &[(&str, u64)] = &[
    ("/de/events", 30),
    ("/de", 24),
    ("/de/events/33rd-roadhouse-birthday-party", 10),
    ("/de/events/25-jahre-revivalband", 8),
    ("/en", 5),
    ("/de/checkout", 5),
    ("/de/checkout/payment", 4),
    ("/de/events/krissy-matthews-band", 4),
    ("/signup", 6),
];
const REFERRERS: &[(Option<&str>, u64)] = &[
    (None, 40),
    (Some("google.com"), 35),
    (Some("instagram.com"), 8),
    (Some("m.facebook.com"), 3),
    (Some("bing.com"), 2),
    (Some("duckduckgo.com"), 1),
    (Some("news.example.net"), 4),
];
const COUNTRIES: &[(&str, u64)] = &[("AT", 70), ("DE", 18), ("CH", 5), ("IT", 4), ("US", 3)];
/// Browser, OS, device, screen.
type Client = (&'static str, &'static str, &'static str, &'static str);
const CLIENTS: &[(Client, u64)] = &[
    (("chrome", "Android OS", "mobile", "412x915"), 35),
    (("ios", "iOS", "mobile", "390x844"), 30),
    (("chrome", "Windows 10", "desktop", "1920x1080"), 15),
    (("firefox", "Windows 10", "desktop", "2560x1440"), 7),
    (("safari", "Mac OS", "desktop", "2560x1600"), 8),
    (("safari", "iOS", "tablet", "820x1180"), 5),
];

#[path = "support/now.rs"]
mod now;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (now, args) = now::from_args()?;
    let mut args = args.into_iter();
    let Some(dir) = args.next() else {
        anyhow::bail!("usage: seed_traffic <data-dir> [days] [--now <instant>]");
    };
    let days: i64 = args.next().map(|d| d.parse()).transpose()?.unwrap_or(45);
    let dir = PathBuf::from(dir);
    let db = Db::open(&dir).await?;
    let sites: Vec<(String, String)> =
        sqlx::query_as("SELECT id, domain FROM sites ORDER BY created_at, id")
            .fetch_all(&db.reader)
            .await?;
    let storage = Storage::open(
        &dir,
        Budgets {
            disk_reserve_bytes: 256 * 1024 * 1024,
            ..Budgets::default()
        },
    )
    .await?;

    let mut rng = Rng(now as u64);
    let mut total = 0;
    for (index, (site, domain)) in sites.iter().enumerate() {
        let policies = session_policies(&db.reader, site)
            .await?
            .ok_or_else(|| anyhow::anyhow!("site {site} has no session policy"))?;
        holocron_analytics::jobs::record_policies(&storage, analytics_id(site)?, policies.clone())
            .await?;
        let site_id = analytics_id(site)?;
        // The first site carries most traffic; the others enough to tell apart.
        let daily: u64 = if index == 0 { 260 } else { 40 };
        let mut records = Vec::new();
        for day in (0..days).rev() {
            let visitors = daily + rng.below(daily / 3 + 1) - daily / 6;
            for visitor in 0..visitors {
                // Recent visitors cluster in the last half hour of today.
                let start = if day == 0 && visitor < 25 {
                    now - rng.below(30) as i64 * MINUTE_MS - rng.below(MINUTE_MS as u64) as i64
                } else {
                    now - day * DAY_MS - rng.below(DAY_MS as u64) as i64
                };
                let identity = policies.identity_day(start);
                let daily_id = Some(u128::from(rng.next()) << 64 | u128::from(rng.next()));
                let (browser, os, device, screen) = *rng.pick(CLIENTS);
                let country = *rng.pick(COUNTRIES);
                let referrer = *rng.pick(REFERRERS);
                let pages = 1 + rng.below(5) as i64;
                // Newsletter and Instagram visits land with campaign parameters, derived from
                // values already drawn, so the rest of the traffic stays the same.
                let campaign = match referrer {
                    Some("news.example.net") => Some((
                        "newsletter",
                        "email",
                        if country == "AT" {
                            "autumn-tour"
                        } else {
                            "birthday-party"
                        },
                    )),
                    Some("instagram.com") => Some(("instagram", "social", "birthday-party")),
                    _ => None,
                };
                for page in 0..pages {
                    let at = (start + page * (20_000 + rng.below(240_000) as i64)).min(now);
                    let landing = campaign.filter(|_| page == 0);
                    let base = Dimensions {
                        hostname: Some(domain.clone()),
                        path: Some(rng.pick(PATHS).to_string()),
                        query: landing.map(|(source, medium, name)| {
                            format!("utm_source={source}&utm_medium={medium}&utm_campaign={name}")
                        }),
                        utm_source: landing.map(|(source, _, _)| source.to_owned()),
                        utm_medium: landing.map(|(_, medium, _)| medium.to_owned()),
                        utm_campaign: landing.map(|(_, _, name)| name.to_owned()),
                        referrer_domain: (page == 0).then(|| referrer.map(str::to_owned)).flatten(),
                        country: Some(country.into()),
                        browser: Some(browser.into()),
                        os: Some(os.into()),
                        device: Some(device.into()),
                        screen: Some(screen.into()),
                        language: Some("de-AT".into()),
                        ..Dimensions::default()
                    };
                    let event = |kind, dimensions, properties, n: usize| {
                        Record::Event(Event {
                            id: u128::from(at as u64) << 64 | n as u128,
                            site: site_id,
                            kind,
                            event_time_ms: at,
                            receive_time_ms: at.max(now - DAY_MS),
                            local_day: identity.local_day(),
                            daily_id,
                            profile_id: None,
                            client_event_id: None,
                            dimensions,
                            performance: None,
                            revenue: None,
                            properties,
                        })
                    };
                    let n = total + records.len();
                    records.push(event(EventKind::Pageview, base.clone(), Vec::new(), n));
                    if page == pages - 1 && rng.below(6) == 0 {
                        let plan = if rng.below(3) == 0 { "free" } else { "pro" };
                        let signup = Dimensions {
                            event_name: Some("signup".into()),
                            ..base
                        };
                        let properties = vec![Property {
                            key: "plan".into(),
                            value: PropertyValue::String(plan.into()),
                        }];
                        records.push(event(EventKind::CustomEvent, signup, properties, n + 1));
                    }
                }
            }
        }
        for chunk in records.chunks(5_000) {
            storage
                .admit(chunk)
                .await
                .map_err(|refusal| anyhow::anyhow!("admission refused: {refusal}"))?;
        }
        total += records.len();
    }
    storage.drain().await;
    let step = run_until_clean(&storage, Limits::default()).await?;
    storage.close().await?;
    db.close().await;
    println!(
        "seeded {total} events over {days} days into {} sites; derived {} sessions",
        sites.len(),
        step.sessions
    );
    Ok(())
}
