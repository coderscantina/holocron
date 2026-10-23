//! Adds the v2 acceptance fixture to a stopped data directory after `seed_demo`,
//! `seed_traffic` and `seed_commerce`: a "Lagoon" team workspace with six sites over three
//! base domains, owned by the admin. Development only; the packaged binary has no equivalent.
//!
//! `cargo run -p holocron-server --example seed_acceptance -- <data-dir> <admin-email> [--now <instant>]`
//!
//! Everything is relative to now and the same for the same instant, so a server on
//! `--frozen-clock` at that instant shows fixed numbers (`docs/evidence/v09-acceptance.md`):
//!
//! - `lagoon.test` has 10 weeks of hourly traffic with a spike on the Tuesday three weeks back,
//!   which the typical-weekday median leaves out. Today a newsletter brings extra visitors from
//!   `news.example.net` to `/de/newsletter/herbst`, half of whom sign up: dimension movers on
//!   both and a goal pace finding on "Signup". It has the goals "Signup" and "Checkout", a
//!   board card with a funnel, replay on, and replays of every tenth newsletter visitor plus
//!   some other visitors of today.
//! - `tickets.lagoon-festival.test` is three weeks old: too little history for a baseline.
//! - The other four sites have 10 weeks of steady traffic.

use std::path::PathBuf;

use chrono::{Datelike, NaiveDate, TimeZone, Weekday};
use chrono_tz::Tz;
use holocron_analytics::{Limits, run_until_clean};
use holocron_protocol::access::{Role, WorkspaceKind};
use holocron_protocol::exploration::{ExplorationState, GoalDefinition};
use holocron_protocol::operations::recordings::RecordingPolicy;
use holocron_server::db::{Db, new_id};
use holocron_server::recordings::replay;
use holocron_server::recordings::store::{Client, Part, Store};
use holocron_server::sites::policy::{analytics_id, session_policies};
use holocron_server::{identity, recordings, sites};
use holocron_storage::{
    Budgets, Dimensions, Event, EventKind, Property, PropertyValue, Record, Storage,
};
use serde_json::{Value, json};

#[path = "support/now.rs"]
mod now;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
const TIMEZONE: &str = "Europe/Vienna";

/// SplitMix64, so every run seeds the same data for the same instant.
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

    /// A factor between `1 - spread` and `1 + spread`.
    fn around(&mut self, spread: f64) -> f64 {
        1.0 - spread + self.below(2000) as f64 / 1000.0 * spread
    }

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

const NEWSLETTER_PATH: &str = "/de/newsletter/herbst";
const NEWSLETTER_REFERRER: &str = "news.example.net";

/// Share of a day's visitors starting in each local hour.
const HOURS: [u64; 24] = [
    3, 2, 1, 1, 1, 2, 4, 7, 10, 13, 15, 16, 17, 17, 18, 19, 20, 21, 22, 21, 18, 14, 9, 5,
];
const HOURS_TOTAL: u64 = {
    let mut sum = 0;
    let mut i = 0;
    while i < HOURS.len() {
        sum += HOURS[i];
        i += 1;
    }
    sum
};
const PATHS: &[(&str, u64)] = &[
    ("/de", 30),
    ("/de/events", 26),
    ("/de/events/seefest", 10),
    ("/de/anreise", 8),
    ("/en", 6),
    ("/de/checkout", 8),
    (NEWSLETTER_PATH, 2),
];
const REFERRERS: &[(Option<&str>, u64)] = &[
    (None, 40),
    (Some("google.com"), 34),
    (Some("instagram.com"), 10),
    (Some("bing.com"), 3),
    (Some(NEWSLETTER_REFERRER), 3),
];
const COUNTRIES: &[(&str, u64)] = &[("AT", 68), ("DE", 20), ("CH", 7), ("IT", 5)];
/// Browser, OS, device, screen.
type Device = (&'static str, &'static str, &'static str, &'static str);
const DEVICES: &[(Device, u64)] = &[
    (("chrome", "Android OS", "mobile", "412x915"), 34),
    (("ios", "iOS", "mobile", "390x844"), 30),
    (("chrome", "Windows 10", "desktop", "1920x1080"), 18),
    (("safari", "Mac OS", "desktop", "2560x1600"), 12),
    (("safari", "iOS", "tablet", "820x1180"), 6),
];

/// One seeded site: name, domain, visitors on a weekday, days of history.
struct SiteSpec {
    name: &'static str,
    domain: &'static str,
    daily: f64,
    days: i64,
    /// Carries the spike, the newsletter, goals, the funnel and replays.
    main: bool,
}

const SITES: [SiteSpec; 6] = [
    SiteSpec {
        name: "Lagoon",
        domain: "lagoon.test",
        daily: 900.0,
        days: 70,
        main: true,
    },
    SiteSpec {
        name: "Lagoon shop",
        domain: "shop.lagoon.test",
        daily: 250.0,
        days: 70,
        main: false,
    },
    SiteSpec {
        name: "Lagoon blog",
        domain: "blog.lagoon.test",
        daily: 120.0,
        days: 70,
        main: false,
    },
    SiteSpec {
        name: "Lagoon Festival",
        domain: "lagoon-festival.test",
        daily: 300.0,
        days: 70,
        main: false,
    },
    SiteSpec {
        name: "Festival tickets",
        domain: "tickets.lagoon-festival.test",
        daily: 150.0,
        days: 21,
        main: false,
    },
    SiteSpec {
        name: "Lagoon Deutschland",
        domain: "lagoon-de.test",
        daily: 90.0,
        days: 70,
        main: false,
    },
];

/// The spike day is the main site's Tuesday three weeks back, at 3.5 times its visitors.
const SPIKE_WEEKS_BACK: i64 = 3;
const SPIKE_FACTOR: f64 = 3.5;
/// Extra newsletter visitors per hour today, from 08:00 local until now.
const NEWSLETTER_PER_HOUR: u64 = 15;
const NEWSLETTER_FROM_HOUR: u32 = 8;
/// Every tenth newsletter visitor today, from the fourth on, was recorded.
const RECORDED_EVERY: usize = 10;
/// Today's visitors without the newsletter who were recorded too, so the filter has to tell.
const OTHER_RECORDINGS: usize = 6;

/// One visitor's visit, kept for the replays of today.
struct Visit {
    daily_id: u128,
    start: i64,
    pages: Vec<(i64, &'static str)>,
    referrer: Option<&'static str>,
    /// Signs up on the last page.
    signs_up: bool,
    newsletter: bool,
    device: Device,
    country: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (now, args) = now::from_args()?;
    let [dir, email] = <[String; 2]>::try_from(args).map_err(|_| {
        anyhow::anyhow!("usage: seed_acceptance <data-dir> <admin-email> [--now <instant>]")
    })?;
    let dir = PathBuf::from(dir);
    let tz: Tz = TIMEZONE.parse().expect("known timezone");
    let db = Db::open(&dir).await?;

    let mut tx = db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let admin: String = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(identity::normalize_email(&email))
        .fetch_one(&mut *tx)
        .await?;
    let admin = holocron_protocol::ids::UserId::new(admin);
    let workspace = sites::create_workspace(&mut tx, "Lagoon", WorkspaceKind::Team).await?;
    sites::add_member(&mut tx, &workspace, &admin, Role::Owner).await?;
    let mut ids = Vec::new();
    for spec in &SITES {
        ids.push(sites::create_site(&mut tx, &workspace, spec.name, spec.domain, TIMEZONE).await?);
    }
    let main = ids[0].as_str().to_owned();
    for (name, predicate) in [
        ("Signup", condition("g1", "event_name", "signup")),
        ("Checkout", condition("g1", "path", "/de/checkout")),
    ] {
        let definition: GoalDefinition = serde_json::from_value(
            json!({ "predicate": predicate, "scope": "session", "counting": "once" }),
        )?;
        let goal = new_id();
        sqlx::query(
            "INSERT INTO goals (id, site_id, name, name_key, version, created_by, created_at, updated_at)
             VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
        )
        .bind(&goal)
        .bind(&main)
        .bind(name)
        .bind(name.to_lowercase())
        .bind(admin.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO goal_versions (goal_id, version, definition, created_by, created_at)
             VALUES (?, 1, ?, ?, ?)",
        )
        .bind(&goal)
        .bind(serde_json::to_string(&definition)?)
        .bind(admin.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    let board = new_id();
    sqlx::query("INSERT INTO boards (id, workspace_id, name, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&board)
        .bind(workspace.as_str())
        .bind("Lagoon")
        .bind(admin.as_str())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    let funnel: ExplorationState = serde_json::from_value(json!({
        "v": 1, "site": main, "measures": [], "dimensions": [], "filter": null,
        "range": { "kind": "relative", "unit": "day", "count": 7 }, "timezone": TIMEZONE,
        "granularity": "day", "compare": "none", "metric_version": 1, "sort": [],
        "page": { "size": 50, "index": 0 }, "visualization": "funnel",
        "report": { "kind": "funnel", "v": 1, "window_seconds": 1800, "scope": "daily", "steps": [
            condition("s1", "path", "/de/events"), condition("s2", "path", "/de/checkout"),
        ]}
    }))?;
    sqlx::query(
        "INSERT INTO board_cards (id, board_id, position, title, site_id, state, exploration_id) VALUES (?, ?, 0, ?, ?, ?, NULL)",
    )
    .bind(new_id())
    .bind(&board)
    .bind("Events to checkout")
    .bind(&main)
    .bind(serde_json::to_string(&funnel)?)
    .execute(&mut *tx)
    .await?;
    let replay_on = RecordingPolicy {
        replay_enabled: true,
        heatmap_enabled: false,
        replay_sample_percent: 10,
        heatmap_sample_percent: 10,
        max_duration_minutes: 5,
        excluded_elements: Vec::new(),
        excluded_paths: Vec::new(),
        version: 1,
    };
    recordings::policy::store(&mut tx, &main, &replay_on, now).await?;
    tx.commit().await?;

    let storage = Storage::open(
        &dir,
        Budgets {
            disk_reserve_bytes: 256 * 1024 * 1024,
            ..Budgets::default()
        },
    )
    .await?;
    let local_now = tz
        .timestamp_millis_opt(now)
        .single()
        .expect("valid instant");
    let today = local_now.date_naive();
    let mut rng = Rng(now as u64 ^ 0xacce97);
    let mut total = 0;
    let mut todays = Vec::new();
    for (spec, id) in SITES.iter().zip(&ids) {
        let policies = session_policies(&db.reader, id.as_str())
            .await?
            .ok_or_else(|| anyhow::anyhow!("site {id} has no session policy"))?;
        let site = analytics_id(id.as_str())?;
        holocron_analytics::jobs::record_policies(&storage, site, policies.clone()).await?;
        let mut records = Vec::new();
        for back in (0..spec.days).rev() {
            let date = today - chrono::Days::new(back as u64);
            let day_factor = match date.weekday() {
                Weekday::Sat | Weekday::Sun => 0.6,
                Weekday::Fri => 0.9,
                _ => 1.0,
            } * if spec.main && back == SPIKE_WEEKS_BACK * 7 {
                SPIKE_FACTOR
            } else {
                1.0
            } * rng.around(0.05);
            for (hour, weight) in HOURS.iter().enumerate() {
                let Some((start, span)) = hour_window(tz, date, hour as u32, now) else {
                    continue;
                };
                let share = spec.daily * day_factor * *weight as f64 / HOURS_TOTAL as f64;
                let visitors = (share * rng.around(0.1) * span as f64 / HOUR_MS as f64).round();
                for _ in 0..visitors as u64 {
                    let at = start + rng.below(span as u64) as i64;
                    let visit = visit(&mut rng, at, now, false);
                    push_visit(
                        &mut records,
                        &mut total,
                        site,
                        spec.domain,
                        &policies,
                        &visit,
                        now,
                    );
                    if spec.main && date == today {
                        todays.push(visit);
                    }
                }
                if spec.main && date == today && hour as u32 >= NEWSLETTER_FROM_HOUR {
                    let extra = NEWSLETTER_PER_HOUR * span as u64 / HOUR_MS as u64;
                    for _ in 0..extra {
                        let at = start + rng.below(span as u64) as i64;
                        let visit = visit(&mut rng, at, now, true);
                        push_visit(
                            &mut records,
                            &mut total,
                            site,
                            spec.domain,
                            &policies,
                            &visit,
                            now,
                        );
                        todays.push(visit);
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
    }
    storage.drain().await;
    let step = run_until_clean(&storage, Limits::default()).await?;
    storage.close().await?;

    let store = Store::open(&dir).await?;
    let site = analytics_id(&main)?;
    let recorded: Vec<&Visit> = todays
        .iter()
        .filter(|v| v.newsletter)
        .skip(3)
        .step_by(RECORDED_EVERY)
        .chain(
            todays
                .iter()
                .filter(|v| !v.matches())
                .step_by(37)
                .take(OTHER_RECORDINGS),
        )
        .collect();
    let matching = recorded.iter().filter(|v| v.matches()).count();
    for visit in &recorded {
        record(&store, site, SITES[0].domain, visit, rng.next()).await?;
    }
    store.close().await;
    db.close().await;
    println!(
        "seeded Lagoon (6 sites, {total} events, {} sessions); {} replays on lagoon.test, {matching} of them from the newsletter",
        step.sessions,
        recorded.len(),
    );
    Ok(())
}

/// A one-condition `and` group.
fn condition(id: &str, field: &str, value: &str) -> Value {
    json!({ "kind": "group", "op": "and", "items": [{
        "kind": "cond", "id": id, "field": field, "cmp": "eq",
        "value": { "t": "string", "v": value }
    }]})
}

/// Start and length of a local hour, cut at now; `None` for an hour after now or one a
/// daylight saving change skips.
fn hour_window(tz: Tz, date: NaiveDate, hour: u32, now: i64) -> Option<(i64, i64)> {
    let start = tz
        .from_local_datetime(&date.and_hms_opt(hour, 0, 0)?)
        .earliest()?
        .timestamp_millis();
    (start < now).then(|| (start, HOUR_MS.min(now - start)))
}

impl Visit {
    /// Matches either mover's filter: from the newsletter's referrer or on its page.
    fn matches(&self) -> bool {
        self.referrer == Some(NEWSLETTER_REFERRER)
            || self.pages.iter().any(|(_, p)| *p == NEWSLETTER_PATH)
    }
}

fn visit(rng: &mut Rng, start: i64, now: i64, newsletter: bool) -> Visit {
    let daily_id = u128::from(rng.next()) << 64 | u128::from(rng.next());
    let device = *rng.pick(DEVICES);
    let country = *rng.pick(COUNTRIES);
    let (referrer, signs_up) = if newsletter {
        (Some(NEWSLETTER_REFERRER), rng.below(2) == 0)
    } else {
        (*rng.pick(REFERRERS), rng.below(6) == 0)
    };
    let count = 1 + rng.below(4) as i64;
    let mut pages = Vec::new();
    for page in 0..count {
        let at = (start + page * (20_000 + rng.below(180_000) as i64)).min(now - 1);
        let path = if newsletter && page == 0 {
            NEWSLETTER_PATH
        } else {
            rng.pick(PATHS)
        };
        pages.push((at, path));
    }
    Visit {
        daily_id,
        start,
        pages,
        referrer,
        signs_up,
        newsletter,
        device,
        country,
    }
}

/// Pageviews of a visit, and a signup at its last page: one in six visitors, every second
/// newsletter visitor.
fn push_visit(
    records: &mut Vec<Record>,
    total: &mut usize,
    site: holocron_storage::Id,
    domain: &str,
    policies: &holocron_analytics::identity::SitePolicies,
    visit: &Visit,
    now: i64,
) {
    let identity = policies.identity_day(visit.start);
    let (browser, os, device, screen) = visit.device;
    for (index, &(at, path)) in visit.pages.iter().enumerate() {
        let base = Dimensions {
            hostname: Some(domain.to_owned()),
            path: Some(path.to_owned()),
            referrer_domain: (index == 0)
                .then(|| visit.referrer.map(str::to_owned))
                .flatten(),
            country: Some(visit.country.into()),
            browser: Some(browser.into()),
            os: Some(os.into()),
            device: Some(device.into()),
            screen: Some(screen.into()),
            language: Some("de-AT".into()),
            ..Dimensions::default()
        };
        let mut event = |kind, dimensions, properties| {
            *total += 1;
            records.push(Record::Event(Event {
                id: u128::from(at as u64) << 64 | *total as u128,
                site,
                kind,
                event_time_ms: at,
                receive_time_ms: at.max(now - DAY_MS),
                local_day: identity.local_day(),
                daily_id: Some(visit.daily_id),
                profile_id: None,
                client_event_id: None,
                dimensions,
                performance: None,
                revenue: None,
                properties,
            }));
        };
        event(EventKind::Pageview, base.clone(), Vec::new());
        if visit.signs_up && index == visit.pages.len() - 1 {
            let signup = Dimensions {
                event_name: Some("signup".into()),
                ..base
            };
            let plan = vec![Property {
                key: "plan".into(),
                value: PropertyValue::String("pro".into()),
            }];
            event(EventKind::CustomEvent, signup, plan);
        }
    }
}

/// One replay of a visit: a meta and a masked full snapshot per page, as the recorder sends.
async fn record(
    store: &Store,
    site: holocron_storage::Id,
    domain: &str,
    visit: &Visit,
    capture: u64,
) -> anyhow::Result<()> {
    let events: Vec<Value> = visit
        .pages
        .iter()
        .flat_map(|&(at, path)| {
            [
                json!({ "type": 4, "timestamp": at,
                    "data": { "href": format!("https://{domain}{path}"), "width": 390, "height": 844 } }),
                json!({ "type": 2, "timestamp": at + 5, "data": {
                    "initialOffset": { "left": 0, "top": 0 },
                    "node": { "type": 0, "id": 1, "childNodes": [
                        { "type": 2, "id": 2, "tagName": "html", "attributes": {}, "childNodes": [
                            { "type": 2, "id": 3, "tagName": "body", "attributes": {}, "childNodes": [
                                { "type": 3, "id": 4, "textContent": "******** ****" }
                            ]}
                        ]}
                    ]}
                }}),
            ]
        })
        .collect();
    let json = serde_json::to_string(&events)?;
    let raw: Vec<Box<serde_json::value::RawValue>> = serde_json::from_str(&json)?;
    let refs: Vec<&serde_json::value::RawValue> = raw.iter().map(AsRef::as_ref).collect();
    let summary = replay::summarize(&refs).map_err(|_| anyhow::anyhow!("invalid replay"))?;
    let (browser, os, device, _) = visit.device;
    let client = Client {
        browser: Some(browser.into()),
        os: Some(os.into()),
        device: Some(device.into()),
        country: Some(visit.country.into()),
    };
    store
        .put(
            Part {
                site,
                capture: u128::from(capture) << 64 | u128::from(visit.daily_id as u64),
                visitor: Some(visit.daily_id),
                chunk: 0,
                received_at: visit.start + 1_000,
                summary: &summary,
                raw_bytes: json.len() as i64,
                payload: zstd::bulk::compress(json.as_bytes(), 3)?,
                fingerprint: blake3::hash(json.as_bytes()).as_bytes()[..16].try_into()?,
                client: &client,
            },
            &[],
            u64::MAX,
        )
        .await?;
    Ok(())
}
