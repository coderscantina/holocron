//! Associated records: retained replay fragments and heatmap interactions,
//! and the burst workload (primary events, fragments, interactions).
//!
//! Interpretations where the manifest is silent are stated next to the code.

use std::net::Ipv6Addr;

use holocron_storage::{
    Dimensions, Event, EventKind, HeatmapInteraction, Interaction, Property, Record,
    ReplayFragment, Revenue,
};

use crate::catalog::{Dimension, fixture_uuid};
use crate::fixture::{Fixture, GROUPS_PER_MONTH, Kind, local_date, month_start};
use crate::load::{Identities, days_since_epoch, property_value};
use crate::prng::{Affine, Stream, mix64, permille, uniform};

pub const FRAGMENT_BYTES: usize = 65_536;

/// Four masked rrweb event templates. A capture's stream cycles through them
/// with node IDs and timestamps from its seed, so content has realistic
/// repetition and is never random padding.
const TEMPLATES: [&str; 4] = [
    r#"{"type":3,"data":{"source":0,"texts":[],"attributes":[],"removes":[],"adds":[{"parentId":{p},"nextId":null,"node":{"type":2,"tagName":"div","attributes":{"class":"card card--{v}"},"childNodes":[{"type":2,"tagName":"h3","attributes":{},"childNodes":[{"type":3,"textContent":"**** ****** **","id":{a}}],"id":{b}},{"type":2,"tagName":"p","attributes":{"class":"muted"},"childNodes":[{"type":3,"textContent":"***** ***** ** ****** *** **** *****","id":{c}}],"id":{d}}],"id":{n}}}]},"timestamp":{t}},"#,
    r#"{"type":3,"data":{"source":1,"positions":[{"x":{x},"y":{y},"id":{n},"timeOffset":-{o}},{"x":{x},"y":{y},"id":{n},"timeOffset":0}]},"timestamp":{t}},"#,
    r#"{"type":3,"data":{"source":5,"text":"********","isChecked":false,"id":{n}},"timestamp":{t}},"#,
    r#"{"type":3,"data":{"source":3,"id":{n},"x":0,"y":{y}},"timestamp":{t}},{"type":3,"data":{"source":2,"type":2,"id":{n},"x":{x},"y":{y},"pointerType":0},"timestamp":{t}},"#,
];

/// The first `len` bytes of a capture's rrweb stream.
pub fn capture_stream(seed: u64, len: usize) -> Vec<u8> {
    let mut out = String::with_capacity(len + 1024);
    out.push_str(r#"[{"type":4,"data":{"href":"https://site.example/","width":1440,"height":900},"timestamp":1790000000000},"#);
    let mut i = 0u64;
    while out.len() < len {
        let u = mix64(seed ^ i);
        let n = 100 + (u % 400);
        let fill = |t: &str| {
            t.replace("{p}", &(n - 1).to_string())
                .replace("{v}", &(u % 7).to_string())
                .replace("{a}", &(n + 1).to_string())
                .replace("{b}", &(n + 2).to_string())
                .replace("{c}", &(n + 3).to_string())
                .replace("{d}", &(n + 4).to_string())
                .replace("{n}", &n.to_string())
                .replace("{x}", &((u >> 16) % 1440).to_string())
                .replace("{y}", &((u >> 32) % 3000).to_string())
                .replace("{o}", &((u >> 8) % 500).to_string())
                .replace("{t}", &(1_790_000_000_000 + i * 120).to_string())
        };
        out.push_str(&fill(TEMPLATES[(u % 4) as usize]));
        i += 1;
    }
    out.truncate(len);
    out.into_bytes()
}

fn fragment(
    id: String,
    site: u128,
    capture: String,
    index: u16,
    total: u16,
    time_ms: i64,
    payload: Vec<u8>,
) -> Record {
    Record::ReplayFragment(ReplayFragment {
        id: fixture_uuid(&id).as_u128(),
        site,
        capture_id: fixture_uuid(&capture).as_u128(),
        index,
        total,
        is_final: index + 1 == total,
        event_time_ms: time_ms,
        receive_time_ms: time_ms,
        payload,
    })
}

/// Placement values of the replay candidates: month 11 groups in its last 14 local days.
pub fn replay_candidates() -> std::ops::Range<u64> {
    (16 * GROUPS_PER_MONTH).div_ceil(30)..GROUPS_PER_MONTH
}

/// Retained replay (associated_data.replay): 50,000 captures of 5 fragments in
/// the last 14 local days of September 2026, for candidates with placement
/// values in `range`. Yields the fragments of each recorded group.
pub fn retained_replay(
    fx: &Fixture,
    range: std::ops::Range<u64>,
) -> impl Iterator<Item = Vec<Record>> + '_ {
    const MONTH: u64 = 11;
    let start = replay_candidates().start;
    let select = Affine::new(GROUPS_PER_MONTH - start, "replay.select");
    range.filter_map(move |v| {
        let recorded = select.value(v - start);
        (recorded < 50_000).then(|| {
            let g = MONTH * GROUPS_PER_MONTH + fx.day.index(v);
            let group = fx.group(g);
            let site = fx.cat.sites[group.site].id.as_u128();
            let stream = capture_stream(g, 5 * FRAGMENT_BYTES);
            (0..5u64)
                .map(|j| {
                    let ordinal = recorded * 5 + j;
                    fragment(
                        format!("fragment:{ordinal}"),
                        site,
                        format!("replay:{g}"),
                        j as u16,
                        5,
                        (group.t0_s + 60 * j as i64 + 1) * 1000,
                        stream[j as usize * FRAGMENT_BYTES..(j as usize + 1) * FRAGMENT_BYTES]
                            .to_vec(),
                    )
                })
                .collect()
        })
    })
}

fn viewport(device: &str) -> (u16, u16) {
    match device {
        "desktop" => (1440, 900),
        "tablet" => (820, 1180),
        _ => (390, 844),
    }
}

/// Retained heatmap interactions: 100,000 per local day for 2026-07-03 to 2026-09-30.
/// `range` selects interaction ordinals so loading can be split.
pub fn retained_heatmap(
    fx: &Fixture,
    range: std::ops::Range<u64>,
) -> impl Iterator<Item = Record> + '_ {
    let stream = Stream::named("heatmap.interaction");
    let first_day = chrono::NaiveDate::from_ymd_opt(2026, 7, 3).unwrap();
    range.map(move |i| {
        let date = first_day + chrono::Days::new(i / 100_000);
        let month = (9..12).find(|m| month_start(*m + 1) > date).unwrap();
        let d = (date - month_start(month)).num_days() as u64;
        let days = crate::fixture::days_in_month(month);
        let day_start = (d * GROUPS_PER_MONTH).div_ceil(days);
        let bucket = ((d + 1) * GROUPS_PER_MONTH).div_ceil(days) - day_start;
        let r = uniform(stream.at(i, 1), bucket);
        let g = month * GROUPS_PER_MONTH + fx.day.index(day_start + r);
        let group = fx.group(g);
        let pageviews: Vec<_> = group
            .events
            .iter()
            .filter(|e| e.kind == Kind::Pageview)
            .collect();
        let pageview = pageviews[uniform(stream.at(i, 2), pageviews.len() as u64) as usize];
        let (w, h) = viewport(fx.cat.device.values[group.device]);
        let kind = if permille(stream.at(i, 0), 800) {
            Interaction::Click {
                x: uniform(stream.at(i, 3), w as u64) as u16,
                page_y: uniform(stream.at(i, 4), 3000) as u32,
            }
        } else {
            Interaction::Scroll {
                percent: uniform(stream.at(i, 3), 101) as u8,
            }
        };
        let time = (pageview.time_s + uniform(stream.at(i, 5), 30) as i64) * 1000;
        Record::HeatmapInteraction(HeatmapInteraction {
            id: fixture_uuid(&format!("interaction:{i}")).as_u128(),
            site: fx.cat.sites[group.site].id.as_u128(),
            capture_id: fixture_uuid(&format!("capture:{g}")).as_u128(),
            event_time_ms: time,
            receive_time_ms: time,
            path: pageview.path.to_owned(),
            viewport_width: w,
            viewport_height: h,
            kind,
        })
    })
}

/// One offered request of the burst: its offset from burst start and its record.
pub struct Offer {
    pub offset_ms: u64,
    pub record: Record,
    /// Primary events are probed for visibility; fragments and interactions are not.
    pub primary: bool,
    /// Fragments carry an empty payload until sent: (capture stream seed, fragment index).
    pub payload: Option<(u64, usize)>,
}

/// The burst manifest: 3,000 groups, event k at 2 ms * k, 300 recorded groups
/// with 10 fragments each, one click per group. `groups` and `spacing_ms`
/// scale the same rules for longer ingestion phases and the 4 events/s baseline.
///
/// Interpretations: dimensions draw from `burst.dims` at ordinal
/// `ordinal * 16 + dimension` with the usual subs; property values and performance
/// metrics use the retained streams at ordinals offset by 10^10, disjoint from
/// the retained fixture. Event time is receive time, so `now_ms` maps offsets
/// to the application clock.
pub fn burst(
    fx: &Fixture,
    ids: &Identities,
    run: u32,
    burst: u32,
    groups: u64,
    start_ms: i64,
    spacing_ms: u64,
) -> Vec<Offer> {
    let cat = fx.cat;
    let events = groups * 10;
    let dims = Stream::named("burst.dims");
    let pick = |d: &Dimension, ordinal: u64, dimension: u64| {
        draw_dimension(d, |sub| dims.at(ordinal * 16 + dimension, sub))
    };
    let pageview_properties = Affine::new(events * 3 / 4, "burst.select_pageview_properties");
    let revenue = Affine::new(events / 5, "burst.select_revenue");
    let recorded = Affine::new(groups, "burst.select_recorded");
    let property_pageviews = events * 3 / 4 * 14_000 / 22_500;
    let revenue_records = events / 5 * 300 / 6_000;
    const OFFSET: u64 = 10_000_000_000;
    let mut offers = Vec::with_capacity((events + groups * 2) as usize);

    for group in 0..groups {
        let site_index = cat.site_of_group(group);
        let site = &cat.sites[site_index];
        let device = pick(&cat.device, group, 0).unwrap();
        let browser = pick(&cat.browser, group, 1).unwrap();
        let os = pick(&cat.os, group, 2).unwrap();
        let country = cat.countries.weights.pick(dims.at(group * 16 + 3, 1));
        let region = uniform(dims.at(group * 16 + 3, 2), cat.regions_per_country);
        let referrer = pick(&cat.referrer, group, 4).map(|r| cat.referrer.values[r]);
        let ip = Ipv6Addr::new(
            0x2001,
            0xdb8,
            0xb,
            0,
            0,
            0,
            (group >> 16) as u16,
            group as u16,
        );
        let user_agent = format!(
            "Mozilla/5.0 (fixture; {}) holocron-fixture/1 {}/{}",
            cat.os.values[os],
            cat.browser.values[browser],
            group % 97
        );
        let time_of = |slot: u64| start_ms + (spacing_ms * (group + groups * slot)) as i64;
        let day = local_date(site.timezone, time_of(0) / 1000);
        let daily_id = ids.anonymous(site_index, day, &ip.to_string(), &user_agent);
        let (mut path, mut query) = ("/", None);
        for slot in 0..10 {
            let e = group * 10 + slot;
            let (r, block) = (e % 20, e / 20);
            let k = group + groups * slot;
            let time = time_of(slot);
            let kind = if r == 9 {
                Kind::Performance
            } else if [3, 7, 13, 17].contains(&r) {
                Kind::Custom
            } else {
                Kind::Pageview
            };
            let mut event_name = None;
            let mut money = None;
            let mut properties = Vec::new();
            match kind {
                Kind::Pageview => {
                    let pv = block * 15
                        + [0, 1, 2, 4, 5, 6, 8, 10, 11, 12, 14, 15, 16, 18, 19]
                            .iter()
                            .position(|x| *x == r)
                            .unwrap() as u64;
                    path = cat.path.values[pick(&cat.path, e, 5).unwrap()];
                    query = pick(&cat.query, e, 6).map(|q| cat.query.values[q]);
                    if pageview_properties.value(pv) < property_pageviews {
                        properties = (0..3).map(|s| fx.drawn_property(OFFSET + e, s)).collect();
                    }
                }
                Kind::Custom => {
                    let c = block * 4 + [3, 7, 13, 17].iter().position(|x| *x == r).unwrap() as u64;
                    let rv = revenue.value(c);
                    properties = (0..3).map(|s| fx.drawn_property(OFFSET + e, s)).collect();
                    if rv < revenue_records {
                        let currency = (rv % 4) as usize;
                        let q = rv / 4;
                        let tier = cat.currencies[currency].tiers[(q % 10) as usize];
                        let amount = if q % 500 >= 490 { -tier } else { tier };
                        event_name = Some("purchase");
                        money = Some(Revenue {
                            currency: cat.currencies[currency].code.to_owned(),
                            minor_units: amount,
                        });
                        let value = if cat.currencies[currency].exponent == 0 {
                            crate::fixture::Value::Int(amount)
                        } else {
                            crate::fixture::Value::Dec(amount * 100)
                        };
                        properties[0] = (
                            cat.key_currency,
                            crate::fixture::Value::Str(cat.currencies[currency].code),
                        );
                        properties[1] = (cat.key_revenue, value);
                    } else {
                        event_name =
                            Some(cat.event_name.values[pick(&cat.event_name, e, 7).unwrap()]);
                    }
                }
                Kind::Performance => {}
            }
            let performance = (kind == Kind::Performance).then(|| {
                let p = fx.performance(OFFSET + block, device);
                let v = |x: Option<i64>| x.map(|x| x as i32);
                holocron_storage::Performance {
                    lcp: v(p[0]),
                    fcp: v(p[1]),
                    ttfb: v(p[2]),
                    inp: v(p[3]),
                    cls: v(p[4]),
                }
            });
            let record = Record::Event(Event {
                id: fixture_uuid(&format!("burst:{run}:{burst}:{k}")).as_u128(),
                site: site.id.as_u128(),
                kind: match kind {
                    Kind::Pageview => EventKind::Pageview,
                    Kind::Custom => EventKind::CustomEvent,
                    Kind::Performance => EventKind::Performance,
                },
                event_time_ms: time,
                receive_time_ms: time,
                local_day: days_since_epoch(day),
                daily_id: Some(daily_id),
                profile_id: None,
                client_event_id: None,
                dimensions: Dimensions {
                    hostname: Some(site.hostname.to_owned()),
                    path: Some(path.to_owned()),
                    query: query.map(str::to_owned),
                    title: Some(path.to_owned()),
                    referrer_domain: referrer.map(str::to_owned),
                    country: Some(cat.countries.values[country].to_owned()),
                    region: Some(format!("{}-R{region}", cat.countries.values[country])),
                    browser: Some(cat.browser.values[browser].to_owned()),
                    os: Some(cat.os.values[os].to_owned()),
                    device: Some(cat.device.values[device].to_owned()),
                    screen: Some(cat.screens[device].to_owned()),
                    language: Some("en-US".to_owned()),
                    event_name: event_name.map(str::to_owned),
                    ..Dimensions::default()
                },
                performance,
                revenue: money,
                properties: properties
                    .iter()
                    .map(|(key, v)| Property {
                        key: cat.keys[*key].name.to_owned(),
                        value: property_value(v),
                    })
                    .collect(),
            });
            offers.push(Offer {
                offset_ms: spacing_ms * k,
                record,
                primary: true,
                payload: None,
            });
        }

        let (w, h) = viewport(cat.device.values[device]);
        let hstream = Stream::named("heatmap.interaction");
        let ordinal = OFFSET + group;
        offers.push(Offer {
            offset_ms: spacing_ms * group + 500,
            primary: false,
            payload: None,
            record: Record::HeatmapInteraction(HeatmapInteraction {
                id: fixture_uuid(&format!("interaction:burst:{run}:{burst}:{group}")).as_u128(),
                site: site.id.as_u128(),
                capture_id: fixture_uuid(&format!("capture:burst:{run}:{burst}:{group}")).as_u128(),
                event_time_ms: time_of(0) + 500,
                receive_time_ms: time_of(0) + 500,
                path: "/".to_owned(),
                viewport_width: w,
                viewport_height: h,
                kind: Interaction::Click {
                    x: uniform(hstream.at(ordinal, 3), w as u64) as u16,
                    page_y: uniform(hstream.at(ordinal, 4), 3000) as u32,
                },
            }),
        });
        if recorded.value(group) < groups / 10 {
            for j in 0..10u64 {
                offers.push(Offer {
                    offset_ms: spacing_ms * (group + groups * j) + 1000,
                    primary: false,
                    payload: Some((mix64(ordinal), j as usize)),
                    record: fragment(
                        format!("fragment:burst:{run}:{burst}:{group}:{j}"),
                        site.id.as_u128(),
                        format!("replay:burst:{run}:{burst}:{group}"),
                        j as u16,
                        10,
                        time_of(j) + 1000,
                        Vec::new(),
                    ),
                });
            }
        }
    }
    offers.sort_by_key(|o| o.offset_ms);
    offers
}

/// `dimension_rules` draws without coverage rows (the burst has none).
fn draw_dimension(d: &Dimension, u: impl Fn(u64) -> u64) -> Option<usize> {
    if d.empty_permille > 0 && permille(u(0), d.empty_permille) {
        return None;
    }
    if d.head_permille > 0 && permille(u(1), d.head_permille) {
        return Some(0);
    }
    Some(d.weights.pick(u(2)))
}
