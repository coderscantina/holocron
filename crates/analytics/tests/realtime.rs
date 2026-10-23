//! Realtime locations: each active identity counts once, at its latest event (0017).

mod common;

use common::*;
use holocron_analytics::realtime::{Location, realtime};
use holocron_storage::EventKind::Pageview;
use holocron_storage::{Lane, Record};

const SITE: u128 = 0x0d2c_9a6e_5a3b_4f1e_9b1c_7e2f_1a4d_8c02;

/// Sets path and location of an event: country, city and coordinates in tenths of a degree.
fn at(
    mut record: Record,
    path: &str,
    country: Option<&str>,
    city: Option<&str>,
    coordinates: Option<(i16, i16)>,
) -> Record {
    let Record::Event(event) = &mut record else {
        unreachable!("only events are built here");
    };
    let d = &mut event.dimensions;
    d.path = Some(path.into());
    d.country = country.map(Into::into);
    d.city = city.map(Into::into);
    (d.latitude, d.longitude) = coordinates.unzip();
    record
}

#[tokio::test(flavor = "multi_thread")]
async fn locations_place_each_active_visitor_at_their_latest_event() {
    let (_dir, storage) = open().await;
    let mut site = Site::new(SITE, chrono_tz::Europe::Vienna);
    let vienna = Some((482, 164));
    let mut event = |time: &str, who: &'static str| site.event(Pageview, time, Who::Anon(who));
    let records = [
        // v1 moved from Graz to Vienna, only Vienna counts, with the latest path.
        at(
            event("2026-06-02T10:06:00Z", "v1"),
            "/a",
            Some("AT"),
            Some("Graz"),
            Some((470, 154)),
        ),
        at(
            event("2026-06-02T10:08:00Z", "v1"),
            "/b",
            Some("AT"),
            Some("Vienna"),
            vienna,
        ),
        at(
            event("2026-06-02T10:07:00Z", "v2"),
            "/c",
            Some("AT"),
            Some("Vienna"),
            vienna,
        ),
        // A country without coordinates lands on its centroid, without a city.
        at(
            event("2026-06-02T10:09:00Z", "v3"),
            "/d",
            Some("AT"),
            Some("Linz"),
            None,
        ),
        // No country, no pin. Idle for more than five minutes, not active.
        at(event("2026-06-02T10:09:00Z", "v4"), "/e", None, None, None),
        at(
            event("2026-06-02T10:04:00Z", "v5"),
            "/f",
            Some("DE"),
            None,
            Some((520, 134)),
        ),
    ];
    ingest(&storage, &records).await;

    let now = ms("2026-06-02T10:10:00Z");
    let live = storage
        .read(Lane::Live, move |s| realtime(s, site.id, now))
        .await
        .unwrap()
        .value;

    assert_eq!(live.active, 4);
    let place = |lat, lng, city: Option<&str>, visitors, path: &str| Location {
        lat,
        lng,
        country: "AT".into(),
        city: city.map(Into::into),
        visitors,
        path: Some(path.into()),
    };
    assert_eq!(
        live.locations,
        [
            place(482, 164, Some("Vienna"), 2, "/b"),
            place(475, 146, None, 1, "/d"),
        ]
    );
    let linz = live
        .recent
        .iter()
        .find(|r| r.path.as_deref() == Some("/d"))
        .unwrap();
    assert_eq!(
        (linz.city.as_deref(), linz.lat, linz.lng),
        (None, Some(475), Some(146))
    );
}
