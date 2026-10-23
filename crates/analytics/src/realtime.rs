//! Realtime snapshot of one site (0003 Realtime): active identities over the last five
//! minutes, 30 one-minute buckets, the latest events and where the active identities are
//! (0017), all by activity time `min(event_time, receive_time)`.

use std::collections::BTreeMap;

use holocron_storage::duckdb::{self, params};
use holocron_storage::{Id, Snapshot};

use crate::country_centroids::centroid;

const MINUTE_MS: i64 = 60_000;
const ACTIVE_MS: i64 = 5 * MINUTE_MS;
pub const BUCKETS: i64 = 30;
pub const RECENT: i64 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realtime {
    pub active: i64,
    /// Bucket start, pageviews, distinct daily pseudonyms; oldest first.
    pub series: Vec<(i64, i64, i64)>,
    pub recent: Vec<Recent>,
    /// Eligible events in the active window with a daily pseudonym or profile, and all of them.
    pub with_identity: i64,
    pub eligible: i64,
    /// Active identities by place, each at its latest event. Most visitors first.
    pub locations: Vec<Location>,
}

/// Active identities in one place: a 0.1 degree cell, or a country centroid for visitors
/// whose CDN sent no coordinates. Degrees are tenths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub lat: i16,
    pub lng: i16,
    pub country: String,
    /// Absent at a country centroid.
    pub city: Option<String>,
    pub visitors: i64,
    /// Path of the latest event in the place.
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub at: i64,
    pub pageview: bool,
    pub path: Option<String>,
    pub event_name: Option<String>,
    pub country: Option<String>,
    /// Absent at a country centroid.
    pub city: Option<String>,
    /// Tenths of a degree; absent when the event has no country, or none with a centroid.
    pub lat: Option<i16>,
    pub lng: Option<i16>,
    pub device: Option<String>,
    pub browser: Option<String>,
}

/// Where to show a visitor: the stored coordinates and city, else the country centroid without
/// a city. `None` without a country, or for a country without a centroid.
fn place(
    country: Option<&str>,
    city: Option<String>,
    lat: Option<i16>,
    lng: Option<i16>,
) -> Option<(i16, i16, Option<String>)> {
    let country = country?;
    match (lat, lng) {
        (Some(lat), Some(lng)) => Some((lat, lng, city)),
        _ => centroid(country).map(|(lat, lng)| (lat, lng, None)),
    }
}

pub fn realtime(snapshot: &Snapshot, site: Id, now_ms: i64) -> duckdb::Result<Realtime> {
    let conn = snapshot.connection();
    let minute = now_ms.div_euclid(MINUTE_MS) * MINUTE_MS;
    let first = minute - (BUCKETS - 1) * MINUTE_MS;
    // Activity time at or after `first` means both times are; the time filters prune storage.
    let window =
        "site = ? AND kind IN (1, 2) AND event_time >= epoch_ms(?) AND receive_time >= epoch_ms(?)
                  AND least(epoch_ms(event_time), epoch_ms(receive_time)) <= ?";

    let (active, with_identity, eligible) = conn.query_row(
        &format!(
            "SELECT count(DISTINCT daily_id),
                    count(*) FILTER (WHERE daily_id IS NOT NULL OR profile_id IS NOT NULL), count(*)
             FROM events WHERE {window}"
        ),
        params![site, now_ms - ACTIVE_MS, now_ms - ACTIVE_MS, now_ms],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    let mut statement = conn.prepare(&format!(
        "SELECT (least(epoch_ms(event_time), epoch_ms(receive_time)) - ?) // {MINUTE_MS} AS bucket,
                count(*) FILTER (WHERE kind = 1), count(DISTINCT daily_id)
         FROM events WHERE {window} GROUP BY 1"
    ))?;
    let mut series: Vec<(i64, i64, i64)> = (0..BUCKETS)
        .map(|i| (first + i * MINUTE_MS, 0, 0))
        .collect();
    let rows = statement.query_map(params![first, site, first, first, now_ms], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (bucket, pageviews, visitors) = row?;
        if let Some(slot) = series.get_mut(bucket as usize) {
            (slot.1, slot.2) = (pageviews, visitors);
        }
    }

    let mut statement = conn.prepare(&format!(
        "SELECT least(epoch_ms(event_time), epoch_ms(receive_time)) AS activity, kind, path, event_name, country, device, browser,
                city, latitude, longitude
         FROM events WHERE {window} ORDER BY activity DESC, seq DESC LIMIT {RECENT}"
    ))?;
    let recent = statement
        .query_map(params![site, first, first, now_ms], |r| {
            let country: Option<String> = r.get(4)?;
            let located = place(country.as_deref(), r.get(7)?, r.get(8)?, r.get(9)?);
            let (lat, lng, city) = match located {
                Some((lat, lng, city)) => (Some(lat), Some(lng), city),
                None => (None, None, None),
            };
            Ok(Recent {
                at: r.get(0)?,
                pageview: r.get::<_, u8>(1)? == 1,
                path: r.get(2)?,
                event_name: r.get(3)?,
                country,
                city,
                lat,
                lng,
                device: r.get(5)?,
                browser: r.get(6)?,
            })
        })?
        .collect::<duckdb::Result<_>>()?;

    // Each identity counts where its latest event in the window put it, then identities group
    // by place. A group's city and path come from its latest identity.
    let mut statement = conn.prepare(&format!(
        "WITH latest AS (
             SELECT country, city, latitude, longitude, path,
                    least(epoch_ms(event_time), epoch_ms(receive_time)) AS activity
             FROM events WHERE {window} AND daily_id IS NOT NULL
             QUALIFY row_number() OVER (PARTITION BY daily_id ORDER BY activity DESC, seq DESC) = 1
         )
         SELECT country, latitude, longitude, city, path, activity,
                count(*) OVER (PARTITION BY country, latitude, longitude)
         FROM latest WHERE country IS NOT NULL
         QUALIFY row_number() OVER (PARTITION BY country, latitude, longitude ORDER BY activity DESC) = 1"
    ))?;
    let groups = statement
        .query_map(
            params![site, now_ms - ACTIVE_MS, now_ms - ACTIVE_MS, now_ms],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i16>>(1)?,
                    r.get::<_, Option<i16>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        )?
        .collect::<duckdb::Result<Vec<_>>>()?;
    let mut places: BTreeMap<(String, i16, i16), (Location, i64)> = BTreeMap::new();
    for (country, lat, lng, city, path, activity, visitors) in groups {
        let Some((lat, lng, city)) = place(Some(&country), city, lat, lng) else {
            continue;
        };
        let key = (country.clone(), lat, lng);
        match places.get_mut(&key) {
            Some((location, latest)) => {
                location.visitors += visitors;
                if (activity, &path, &city) > (*latest, &location.path, &location.city) {
                    (location.city, location.path, *latest) = (city, path, activity);
                }
            }
            None => {
                let location = Location {
                    lat,
                    lng,
                    country,
                    city,
                    visitors,
                    path,
                };
                places.insert(key, (location, activity));
            }
        }
    }
    let mut locations: Vec<Location> = places.into_values().map(|(location, _)| location).collect();
    locations.sort_by(|a, b| {
        b.visitors
            .cmp(&a.visitors)
            .then_with(|| (&a.country, a.lat, a.lng).cmp(&(&b.country, b.lat, b.lng)))
    });

    Ok(Realtime {
        active,
        series,
        recent,
        with_identity,
        eligible,
        locations,
    })
}
