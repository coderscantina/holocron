//! The reconciliation manifest: source and destination counts per site and kind, properties,
//! date ranges, revenue per currency, configuration objects, sampled events, and digests of the
//! destination that a resumed import must reproduce exactly.
//!
//! Source counts come from aggregate queries over the same snapshot the import read, filtered by
//! the same retention cutoffs. Destination counts come from the stores. Every source object of a
//! configuration table is either imported under its ID or in the inventory; anything else is
//! listed as unaccounted.

use std::collections::{BTreeMap, BTreeSet};

use holocron_protocol::exploration::currency_exponent;
use holocron_storage::Lane;
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use super::control::login_email;
use super::events::uuid;
use super::signature::Layout;
use super::source::{Arg, Kind, RowExt};
use super::{Ctx, derived_id, state};
use crate::collect::decimal::Decimal;
use crate::init::Failure;
use crate::recordings::{self, heatmaps, store::RECORDINGS_DB_FILE};

/// Events compared field by field with their source rows.
const SAMPLE: i64 = 50;

/// Event kind codes as stored (`EventKind`) and their manifest names.
const KINDS: [(i64, u8, &str); 5] = [
    (1, 1, "pageview"),
    (2, 2, "custom_event"),
    (5, 3, "performance"),
    (3, 4, "link_event"),
    (4, 5, "pixel_event"),
];

pub async fn manifest(ctx: &mut Ctx) -> Result<Value, Failure> {
    let cutoffs = ctx.cutoffs();
    let mut differences: Vec<String> = Vec::new();
    let targets = targets(ctx).await?;

    // Events by target and kind.
    let e = ctx.source.engine;
    let groups = ctx
        .source
        .rows(
            &format!(
                "SELECT {}, {}, {}, {}, count(*), {}, {} FROM website_event GROUP BY 1, 2, 3, 4",
                e.text("website_id"),
                e.int("event_type"),
                e.flag(&format!("created_at >= {}", e.ms_param())),
                e.flag(&format!("created_at >= {}", e.ms_param())),
                e.ms("min(created_at)"),
                e.ms("max(created_at)"),
            ),
            &[Arg::Int(cutoffs.site_ms), Arg::Int(cutoffs.utc_ms)],
        )
        .await?;
    let mut source: BTreeMap<(String, &str), (i64, i64, i64)> = BTreeMap::new();
    let mut outside: BTreeMap<String, i64> = BTreeMap::new();
    let mut not_imported = BTreeMap::from([
        ("no_target", 0i64),
        ("no_timestamp", 0),
        ("kind_mismatch", 0),
    ]);
    for row in &groups {
        let site: String = row.try_get(0).map_err(super::source_error)?;
        let kind: i64 = row.try_get(1).map_err(super::source_error)?;
        let (in_site, in_utc): (Option<i64>, Option<i64>) = (
            row.try_get(2).map_err(super::source_error)?,
            row.try_get(3).map_err(super::source_error)?,
        );
        let count: i64 = row.try_get(4).map_err(super::source_error)?;
        let (first, last): (Option<i64>, Option<i64>) = (
            row.try_get(5).map_err(super::source_error)?,
            row.try_get(6).map_err(super::source_error)?,
        );
        let Some(target) = targets.get(&site) else {
            *not_imported.get_mut("no_target").expect("key") += count;
            continue;
        };
        let Some(name) = kind_name(kind, target) else {
            *not_imported.get_mut("kind_mismatch").expect("key") += count;
            continue;
        };
        let in_retention = if *target == "site" { in_site } else { in_utc };
        match in_retention {
            None => *not_imported.get_mut("no_timestamp").expect("key") += count,
            Some(0) => *outside.entry(site).or_default() += count,
            Some(_) => {
                let entry = source
                    .entry((site, name))
                    .or_insert((0, i64::MAX, i64::MIN));
                entry.0 += count;
                entry.1 = entry.1.min(first.unwrap_or(i64::MAX));
                entry.2 = entry.2.max(last.unwrap_or(i64::MIN));
            }
        }
    }

    let destination = ctx
        .storage
        .read(Lane::Report, |snapshot| {
            let conn = snapshot.connection();
            let events = conn
                .prepare(
                    "SELECT site::VARCHAR, kind, count(*), epoch_ms(min(event_time)), epoch_ms(max(event_time))
                     FROM events GROUP BY ALL",
                )?
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, u8>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let by_site = |sql: &str| -> holocron_storage::duckdb::Result<BTreeMap<String, i64>> {
                conn.prepare(sql)?
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                    .collect()
            };
            let properties = by_site("SELECT site::VARCHAR, count(*) FROM event_properties GROUP BY ALL")?;
            let identify = by_site("SELECT site::VARCHAR, count(*) FROM session_properties GROUP BY ALL")?;
            let heatmaps = by_site("SELECT site::VARCHAR, count(*) FROM heatmap_interactions GROUP BY ALL")?;
            let sessions = by_site("SELECT site::VARCHAR, count(*) FROM sessions GROUP BY ALL")?;
            let visitor_days = by_site(
                "SELECT site::VARCHAR, count(*) FROM (SELECT DISTINCT site, local_day, daily_id FROM events
                 WHERE daily_id IS NOT NULL AND kind IN (1, 2)) GROUP BY ALL",
            )?;
            let revenue = conn
                .prepare(
                    "SELECT site::VARCHAR, currency, sum(revenue_minor)::BIGINT FROM events
                     WHERE revenue_minor IS NOT NULL GROUP BY ALL",
                )?
                .query_map([], |r| Ok(((r.get::<_, String>(0)?, r.get::<_, String>(1)?), r.get::<_, i64>(2)?)))?
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            let digest = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, String>(0));
            let digests = json!({
                "events": digest("SELECT concat(count(*), ':', coalesce(sum(hash(id, site, kind, event_time, local_day, daily_id,
                    hostname, path, query, title, referrer_domain, referrer_path, utm_source, utm_medium, utm_campaign,
                    utm_content, utm_term, click_id, country, region, city, browser, os, device, screen, language,
                    event_name, tag, lcp, fcp, ttfb, inp, cls, currency, revenue_minor)), 0)) FROM events")?,
                "properties": digest("SELECT concat(count(*), ':', coalesce(sum(hash(e.id, p.key, p.type, p.v_string, p.v_integer,
                    p.v_decimal, p.v_bool, p.v_date, p.v_array)), 0))
                    FROM event_properties p JOIN events e ON e.seq = p.seq")?,
                "identify": digest("SELECT concat(count(*), ':', coalesce(sum(hash(id, site, event_time, local_day, daily_id, key,
                    type, v_string, v_integer, v_decimal, v_bool, v_date, v_array)), 0)) FROM session_properties")?,
                "sessions": digest("SELECT concat(count(*), ':', coalesce(sum(hash(site, population, identity, local_day, start_time,
                    last_time, pageviews, custom_events, policy_version, country, browser, referrer_domain, utm_source)), 0))
                    FROM sessions")?,
                "heatmaps": digest("SELECT concat(count(*), ':', coalesce(sum(hash(id, site, capture_id, event_time, path,
                    viewport_width, viewport_height, kind, x, page_y, scroll_percent)), 0)) FROM heatmap_interactions")?,
                "provenance": digest("SELECT concat(count(*), ':', count(DISTINCT event_id), ':', coalesce(sum(hash(event_id,
                    site, source_session, source_visit, distinct_id)), 0)) FROM import_provenance")?,
            });
            Ok(Destination { events, properties, identify, heatmaps, sessions, visitor_days, revenue, digests })
        })
        .await
        .map_err(super::internal)?
        .value;

    // Properties, identify data and heatmaps in retention, by site.
    let properties = by_site(
        ctx,
        &format!(
            "SELECT {}, count(*) FROM event_data d JOIN website_event e ON e.event_id = d.website_event_id
             WHERE e.created_at >= {} GROUP BY 1",
            e.text("e.website_id"),
            e.ms_param()
        ),
        cutoffs.site_ms,
    )
    .await?;
    let identify = by_site(
        ctx,
        &format!(
            "SELECT {}, count(*) FROM session_data WHERE created_at >= {} GROUP BY 1",
            e.text("website_id"),
            e.ms_param()
        ),
        cutoffs.site_ms,
    )
    .await?;
    let heatmap_source = if ctx.layout == Layout::Umami3Postgresql {
        by_site(
            ctx,
            &format!(
                "SELECT {}, count(*) FROM heatmap_event WHERE created_at >= {} AND viewport_w >= 1
                   AND ((event_type = 1 AND page_x IS NOT NULL AND page_y IS NOT NULL)
                        OR (event_type = 2 AND scroll_pct IS NOT NULL)) GROUP BY 1",
                e.text("website_id"),
                e.ms_param()
            ),
            cutoffs.heatmap_ms,
        )
        .await?
    } else {
        BTreeMap::new()
    };
    let revenue_source = ctx
        .source
        .rows(
            &format!(
                "SELECT {}, {}, {} FROM revenue WHERE created_at >= {} GROUP BY 1, 2",
                e.text("website_id"),
                e.text("upper(currency)"),
                e.text("sum(revenue)"),
                e.ms_param()
            ),
            &[Arg::Int(cutoffs.site_ms)],
        )
        .await?;
    let mut revenue: BTreeMap<(String, String), (Option<i64>, Option<i64>)> = BTreeMap::new();
    for row in &revenue_source {
        let site: String = row.try_get(0).map_err(super::source_error)?;
        let currency: String = row.try_get(1).map_err(super::source_error)?;
        let total: Option<String> = row.try_get(2).map_err(super::source_error)?;
        if !targets.contains_key(&site) {
            continue;
        }
        let minor = currency_exponent(&currency).and_then(|exponent| {
            Decimal::parse(total.as_deref().unwrap_or("0"))?
                .scaled(u32::from(exponent))
                .and_then(|v| i64::try_from(v).ok())
        });
        revenue.entry((site, currency)).or_default().0 = minor;
    }
    for ((site, currency), minor) in &destination.revenue {
        revenue
            .entry((dashed(site), currency.clone()))
            .or_default()
            .1 = Some(*minor);
    }

    // Per target report.
    let mut destination_events: BTreeMap<(String, &str), (i64, i64, i64)> = BTreeMap::new();
    for (site, kind, count, first, last) in &destination.events {
        let name = KINDS
            .iter()
            .find(|k| k.1 == *kind)
            .map_or("unknown", |k| k.2);
        destination_events.insert((dashed(site), name), (*count, *first, *last));
    }
    let names: BTreeMap<String, String> = sqlx::query_as(
        "SELECT id, name FROM sites UNION ALL SELECT id, name FROM links UNION ALL SELECT id, name FROM pixels",
    )
    .fetch_all(&ctx.db.reader)
    .await
    .map_err(super::internal)?
    .into_iter()
    .collect();
    let replays = replays(ctx, &targets, cutoffs.replay_ms).await?;
    let mut sites = Vec::new();
    for (id, target) in &targets {
        let mut kinds = serde_json::Map::new();
        let mut first = i64::MAX;
        let mut last = i64::MIN;
        for (_, _, name) in KINDS {
            let src = source.get(&(id.clone(), name)).copied();
            let dst = destination_events.get(&(id.clone(), name)).copied();
            if src.is_none() && dst.is_none() {
                continue;
            }
            let (src_count, src_first, src_last) = src.unwrap_or((0, i64::MAX, i64::MIN));
            let (dst_count, dst_first, dst_last) = dst.unwrap_or((0, i64::MAX, i64::MIN));
            if src_count != dst_count
                || (src_count > 0 && (src_first, src_last) != (dst_first, dst_last))
            {
                differences.push(format!(
                    "{target} {id} {name}: source {src_count} events {src_first}..{src_last}, destination {dst_count} {dst_first}..{dst_last}"
                ));
            }
            first = first.min(dst_first);
            last = last.max(dst_last);
            kinds.insert(
                name.into(),
                json!({ "source": src_count, "destination": dst_count,
                        "source_range": [src_first, src_last], "destination_range": [dst_first, dst_last] }),
            );
        }
        let key = undashed(id);
        let compare = |what: &str, source: i64, destination: i64, differences: &mut Vec<String>| {
            if source != destination {
                differences.push(format!(
                    "{target} {id} {what}: source {source}, destination {destination}"
                ));
            }
            json!({ "source": source, "destination": destination })
        };
        let mut report = json!({
            "id": id,
            "kind": target,
            "name": names.get(id),
            "events": kinds,
            "outside_retention": outside.get(id).copied().unwrap_or(0),
            "properties": compare("properties", properties.get(id).copied().unwrap_or(0),
                                  destination.properties.get(&key).copied().unwrap_or(0), &mut differences),
        });
        if *target == "site" {
            report["identify"] = compare(
                "identify values",
                identify.get(id).copied().unwrap_or(0),
                destination.identify.get(&key).copied().unwrap_or(0),
                &mut differences,
            );
            report["heatmaps"] = compare(
                "heatmap interactions",
                heatmap_source.get(id).copied().unwrap_or(0),
                destination.heatmaps.get(&key).copied().unwrap_or(0),
                &mut differences,
            );
            let (src, stored, inventoried) = replays.get(id).copied().unwrap_or((0, 0, 0));
            if src != stored + inventoried {
                differences.push(format!(
                    "site {id} replay parts: source {src}, stored {stored}, inventoried {inventoried}"
                ));
            }
            report["replay_parts"] =
                json!({ "source": src, "destination": stored, "inventoried": inventoried });
            report["sessions"] = json!(destination.sessions.get(&key).copied().unwrap_or(0));
            report["visitor_days"] =
                json!(destination.visitor_days.get(&key).copied().unwrap_or(0));
            let currencies: Vec<Value> = revenue
                .iter()
                .filter(|((site, _), _)| site == id)
                .map(|((_, currency), (src, dst))| {
                    if src != dst {
                        differences.push(format!(
                            "site {id} revenue {currency}: source revenue table {src:?}, destination {dst:?} minor units"
                        ));
                    }
                    json!({ "currency": currency, "source_revenue_table_minor": src, "destination_minor": dst })
                })
                .collect();
            report["revenue"] = currencies.into();
        }
        if first <= last {
            report["destination_range"] = json!([first, last]);
        }
        sites.push(report);
    }

    let configuration = configuration(ctx, &mut differences).await?;
    let samples = samples(ctx, &targets, &cutoffs, &mut differences).await?;
    for (reason, count) in &not_imported {
        if *count > 0 {
            differences.push(format!("{count} events not imported: {reason}"));
        }
    }
    Ok(json!({
        "source": ctx.layout.source_name(),
        "clock": ctx.clock_ms,
        "options": ctx.options,
        "retention": {
            "events_from": cutoffs.site_ms,
            "link_and_pixel_events_from": cutoffs.utc_ms,
            "unsaved_replays_from": cutoffs.replay_ms,
            "heatmaps_from": cutoffs.heatmap_ms,
        },
        "status": if differences.is_empty() { "reconciled" } else { "differences" },
        "differences": differences,
        "sites": sites,
        "events_not_imported": not_imported,
        "configuration": configuration,
        "logins": logins(ctx).await?,
        "samples": samples,
        "inventory": state::inventory_counts(&ctx.db).await.map_err(super::internal)?,
        "steps": state::steps(&ctx.db).await.map_err(super::internal)?,
        "digests": destination.digests,
    }))
}

struct Destination {
    events: Vec<(String, u8, i64, i64, i64)>,
    properties: BTreeMap<String, i64>,
    identify: BTreeMap<String, i64>,
    heatmaps: BTreeMap<String, i64>,
    sessions: BTreeMap<String, i64>,
    visitor_days: BTreeMap<String, i64>,
    revenue: BTreeMap<(String, String), i64>,
    digests: Value,
}

/// The retention cutoffs an import applies, at its pinned clock.
pub struct Cutoffs {
    pub site_ms: i64,
    pub utc_ms: i64,
    pub replay_ms: i64,
    pub heatmap_ms: i64,
}

impl Ctx {
    pub fn cutoffs(&self) -> Cutoffs {
        use holocron_analytics::identity::{SessionPolicy, SitePolicies};
        let policies = |tz| SitePolicies::new(SessionPolicy::new(tz, 30).expect("default window"));
        Cutoffs {
            site_ms: policies(self.timezone)
                .retention_floor_ms(self.clock_ms, self.options.retention),
            utc_ms: policies(chrono_tz::UTC)
                .retention_floor_ms(self.clock_ms, self.options.retention),
            replay_ms: recordings::replay_cutoff_ms(self.timezone, self.clock_ms),
            heatmap_ms: i64::from(heatmaps::first_retained_day(self.clock_ms)) * 86_400_000,
        }
    }
}

/// Imported sites, links and pixels by source ID.
async fn targets(ctx: &Ctx) -> Result<BTreeMap<String, &'static str>, Failure> {
    let mut targets = BTreeMap::new();
    for (table, kind) in [("sites", "site"), ("links", "link"), ("pixels", "pixel")] {
        let ids: Vec<String> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {table}")))
                .fetch_all(&ctx.db.reader)
                .await
                .map_err(super::internal)?;
        targets.extend(ids.into_iter().map(|id| (id, kind)));
    }
    Ok(targets)
}

fn kind_name(event_type: i64, target: &str) -> Option<&'static str> {
    let name = KINDS.iter().find(|k| k.0 == event_type)?.2;
    let fits = match target {
        "site" => matches!(event_type, 1 | 2 | 5),
        "link" => event_type == 3,
        _ => event_type == 4,
    };
    fits.then_some(name)
}

/// DuckDB prints a `UHUGEINT` site as a decimal integer; the source has a hyphenated UUID.
fn dashed(site: &str) -> String {
    site.parse::<u128>()
        .map(|n| uuid::Uuid::from_u128(n).hyphenated().to_string())
        .unwrap_or_default()
}

fn undashed(id: &str) -> String {
    uuid(id).to_string()
}

async fn by_site(ctx: &mut Ctx, sql: &str, from_ms: i64) -> Result<BTreeMap<String, i64>, Failure> {
    ctx.source
        .rows(sql, &[Arg::Int(from_ms)])
        .await?
        .iter()
        .map(|r| Ok((r.try_get(0)?, r.try_get(1)?)))
        .collect::<Result<_, sqlx::Error>>()
        .map_err(super::source_error)
}

/// Source replay parts in retention or saved, stored parts, and inventoried parts, by site.
async fn replays(
    ctx: &mut Ctx,
    targets: &BTreeMap<String, &str>,
    from_ms: i64,
) -> Result<BTreeMap<String, (i64, i64, i64)>, Failure> {
    let mut out: BTreeMap<String, (i64, i64, i64)> = BTreeMap::new();
    if ctx.layout != Layout::Umami3Postgresql {
        return Ok(out);
    }
    let e = ctx.source.engine;
    let source = ctx
        .source
        .rows(
            &format!(
                "SELECT {}, count(*) FROM session_replay r
                 WHERE r.started_at >= {} OR EXISTS (SELECT 1 FROM session_replay_saved v
                     WHERE v.website_id = r.website_id AND v.visit_id = r.visit_id)
                 GROUP BY 1",
                e.text("r.website_id"),
                e.ms_param()
            ),
            &[Arg::Int(from_ms)],
        )
        .await?;
    for row in &source {
        let site: String = row.try_get(0).map_err(super::source_error)?;
        if targets.contains_key(&site) {
            out.entry(site).or_default().0 = row.try_get(1).map_err(super::source_error)?;
        }
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(ctx.data_dir.join(RECORDINGS_DB_FILE))
                .read_only(true),
        )
        .await
        .map_err(super::internal)?;
    let stored: Vec<(Vec<u8>, i64)> =
        sqlx::query_as("SELECT site, count(*) FROM chunks GROUP BY site")
            .fetch_all(&pool)
            .await
            .map_err(super::internal)?;
    pool.close().await;
    for (site, count) in stored {
        let site = uuid::Uuid::from_bytes(site.try_into().unwrap_or([0; 16]))
            .hyphenated()
            .to_string();
        out.entry(site).or_default().1 = count;
    }
    let inventoried: Vec<(String, i64)> = sqlx::query_as(
        "SELECT json_extract(representation, '$.row.website_id'), count(*) FROM import_inventory
         WHERE source_table = 'session_replay' GROUP BY 1",
    )
    .fetch_all(&ctx.db.reader)
    .await
    .map_err(super::internal)?;
    for (site, count) in inventoried {
        out.entry(site).or_default().2 = count;
    }
    Ok(out)
}

/// Every source object of a configuration table, imported under its ID or inventoried.
async fn configuration(ctx: &mut Ctx, differences: &mut Vec<String>) -> Result<Value, Failure> {
    let pg = ctx.layout == Layout::Umami3Postgresql;
    let mut tables: Vec<(&str, &str, Option<&str>)> = vec![
        ("user", "user_id", Some("users")),
        ("team", "team_id", Some("workspaces")),
        ("team_user", "team_user_id", None),
        ("website", "website_id", Some("sites")),
        ("segment", "segment_id", Some("segments")),
        ("report", "report_id", Some("saved_explorations")),
    ];
    if pg {
        tables.extend([
            ("link", "link_id", Some("links")),
            ("pixel", "pixel_id", Some("pixels")),
            ("board", "board_id", Some("boards")),
            ("annotation", "annotation_id", Some("annotations")),
            ("share", "share_id", Some("shares")),
            ("two_factor_auth", "id", None),
            ("api_key", "api_key_id", None),
            ("two_factor_backup_code", "id", None),
            ("two_factor_otp_used", "id", None),
            ("two_factor_rate_limit", "id", None),
        ]);
    }
    let mut out = serde_json::Map::new();
    for (table, key, destination) in tables {
        let ids: BTreeSet<String> = ctx
            .source
            .table(table, &[(key, Kind::Text)], "", key, &[])
            .await?
            .iter()
            .map(|r| r.text(key).unwrap_or_default().to_owned())
            .collect();
        let imported: BTreeSet<String> = match destination {
            Some(dest) => sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
                "SELECT id FROM {dest}"
            )))
            .fetch_all(&ctx.db.reader)
            .await
            .map_err(super::internal)?
            .into_iter()
            .filter(|id| ids.contains(id))
            .collect(),
            None => match table {
                "team_user" => {
                    // A membership has no ID of its own: count the source rows whose pair exists.
                    let pairs: BTreeSet<(String, String)> =
                        sqlx::query_as("SELECT workspace_id, user_id FROM memberships")
                            .fetch_all(&ctx.db.reader)
                            .await
                            .map_err(super::internal)?
                            .into_iter()
                            .collect();
                    ctx.source
                        .table(
                            table,
                            &[
                                (key, Kind::Text),
                                ("team_id", Kind::Text),
                                ("user_id", Kind::Text),
                            ],
                            "",
                            key,
                            &[],
                        )
                        .await?
                        .iter()
                        .filter(|r| {
                            pairs.contains(&(
                                r.text("team_id").unwrap_or_default().to_owned(),
                                r.text("user_id").unwrap_or_default().to_owned(),
                            ))
                        })
                        .map(|r| r.text(key).unwrap_or_default().to_owned())
                        .collect()
                }
                "two_factor_auth" => {
                    let kept: BTreeSet<String> =
                        sqlx::query_scalar("SELECT user_id FROM two_factor")
                            .fetch_all(&ctx.db.reader)
                            .await
                            .map_err(super::internal)?
                            .into_iter()
                            .collect();
                    ctx.source
                        .table(
                            table,
                            &[(key, Kind::Text), ("user_id", Kind::Text)],
                            "",
                            key,
                            &[],
                        )
                        .await?
                        .iter()
                        .filter(|r| kept.contains(r.text("user_id").unwrap_or_default()))
                        .map(|r| r.text(key).unwrap_or_default().to_owned())
                        .collect()
                }
                _ => BTreeSet::new(),
            },
        };
        let inventoried: BTreeSet<String> = sqlx::query_scalar(
            "SELECT DISTINCT source_key FROM import_inventory WHERE source_table = ?",
        )
        .bind(table)
        .fetch_all(&ctx.db.reader)
        .await
        .map_err(super::internal)?
        .into_iter()
        .filter(|k: &String| ids.contains(k))
        .collect();
        let unaccounted: Vec<&String> = ids
            .iter()
            .filter(|id| !imported.contains(*id) && !inventoried.contains(*id))
            .collect();
        if !unaccounted.is_empty() {
            differences.push(format!(
                "{table}: {} rows neither imported nor inventoried",
                unaccounted.len()
            ));
        }
        out.insert(
            table.into(),
            json!({ "source": ids.len(), "imported": imported.len(),
                    "inventoried": inventoried.len(), "unaccounted": unaccounted }),
        );
    }
    if !pg {
        // 2.x public links live on the website row.
        let shares = ctx
            .source
            .table(
                "website",
                &[("website_id", Kind::Text)],
                "share_id IS NOT NULL",
                "website_id",
                &[],
            )
            .await?;
        let imported: BTreeSet<String> = sqlx::query_scalar("SELECT id FROM shares")
            .fetch_all(&ctx.db.reader)
            .await
            .map_err(super::internal)?
            .into_iter()
            .collect();
        let count = shares
            .iter()
            .filter(|r| {
                imported.contains(&derived_id(
                    "share",
                    r.text("website_id").unwrap_or_default(),
                ))
            })
            .count();
        out.insert(
            "website.share_id".into(),
            json!({ "source": shares.len(), "imported": count }),
        );
    }
    Ok(out.into())
}

/// The login of every imported account and what happened to its credentials.
async fn logins(ctx: &mut Ctx) -> Result<Value, Failure> {
    let users = ctx
        .source
        .table(
            "user",
            &[("user_id", Kind::Text), ("username", Kind::Text)],
            "",
            "user_id",
            &[],
        )
        .await?;
    let accounts: BTreeMap<String, (String, String, bool, bool)> = sqlx::query_as(
        "SELECT u.id, u.email, u.password_hash, t.confirmed_at IS NOT NULL, u.two_factor_required
         FROM users u LEFT JOIN two_factor t ON t.user_id = u.id",
    )
    .fetch_all(&ctx.db.reader)
    .await
    .map_err(super::internal)?
    .into_iter()
    .map(
        |(id, email, hash, enrolled, required): (String, String, String, bool, bool)| {
            (id, (email, hash, enrolled, required))
        },
    )
    .collect();
    Ok(users
        .iter()
        .filter_map(|u| {
            let (email, hash, enrolled, required) = accounts.get(u.text("user_id")?)?;
            let username = u.text("username").unwrap_or_default();
            debug_assert_eq!(email, &login_email(username, &ctx.options.login_domain));
            Some(json!({
                "username": username,
                "email": email,
                "password": if hash.starts_with("$2") { "kept" } else { "reset_required" },
                "two_factor": match (enrolled, required) {
                    (true, _) => "kept",
                    (false, true) => "enrollment_required",
                    (false, false) => "none",
                },
            }))
        })
        .collect::<Vec<_>>()
        .into())
}

/// The first imported events by ID, compared field by field with their source rows.
async fn samples(
    ctx: &mut Ctx,
    targets: &BTreeMap<String, &str>,
    cutoffs: &Cutoffs,
    differences: &mut Vec<String>,
) -> Result<Value, Failure> {
    let e = ctx.source.engine;
    let fields = [
        "url_path",
        "page_title",
        "referrer_domain",
        "event_name",
        "hostname",
        "utm_source",
    ];
    let columns: Vec<String> = fields.iter().map(|f| e.text(&format!("e.{f}"))).collect();
    let rows = ctx
        .source
        .rows(
            &format!(
                "SELECT {}, {}, {}, {}, {}, {}, {} FROM website_event e LEFT JOIN session s ON s.session_id = e.session_id
                 WHERE e.created_at >= {} ORDER BY e.event_id LIMIT ?",
                e.text("e.event_id"),
                e.text("e.website_id"),
                e.int("e.event_type"),
                e.ms("e.created_at"),
                columns.join(", "),
                e.text("s.browser"),
                e.text("s.country"),
                e.ms_param(),
            ),
            &[Arg::Int(cutoffs.site_ms.max(cutoffs.utc_ms)), Arg::Int(SAMPLE * 4)],
        )
        .await?;
    let mut expected = Vec::new();
    for row in &rows {
        let site: String = row.try_get(1).map_err(super::source_error)?;
        let kind: i64 = row.try_get(2).map_err(super::source_error)?;
        let Some(target) = targets.get(&site) else {
            continue;
        };
        if kind_name(kind, target).is_none() {
            continue;
        }
        let mut values = Vec::new();
        for i in 3..=11 {
            values.push(
                match i {
                    3 => row.try_get::<Option<i64>, _>(i).map(|v| json!(v)),
                    _ => row.try_get::<Option<String>, _>(i).map(|v| json!(v)),
                }
                .map_err(super::source_error)?,
            );
        }
        expected.push((
            row.try_get::<String, _>(0).map_err(super::source_error)?,
            values,
        ));
        if expected.len() as i64 == SAMPLE {
            break;
        }
    }
    let ids: Vec<u128> = expected.iter().map(|(id, _)| uuid(id)).collect();
    let found = ctx
        .storage
        .read(Lane::Report, move |snapshot| {
            let conn = snapshot.connection();
            let mut statement = conn.prepare(
                "SELECT epoch_ms(event_time), path, title, referrer_domain, event_name, hostname, utm_source,
                        browser, country FROM events WHERE id = ?",
            )?;
            ids.iter()
                .map(|id| {
                    let rows: Vec<Vec<Value>> = statement
                        .query_map([*id], |r| {
                            let mut values = vec![json!(r.get::<_, i64>(0)?)];
                            for i in 1..9 {
                                values.push(json!(r.get::<_, Option<String>>(i)?));
                            }
                            Ok(values)
                        })?
                        .collect::<Result<_, _>>()?;
                    Ok(rows)
                })
                .collect::<holocron_storage::duckdb::Result<Vec<_>>>()
        })
        .await
        .map_err(super::internal)?
        .value;
    let mut mismatches = Vec::new();
    for ((id, values), rows) in expected.iter().zip(&found) {
        let ok = rows.len() == 1 && rows[0] == *values;
        if !ok {
            mismatches.push(json!({ "event_id": id, "source": values, "destination": rows }));
        }
    }
    if !mismatches.is_empty() {
        differences.push(format!(
            "{} of {} sampled events differ",
            mismatches.len(),
            expected.len()
        ));
    }
    Ok(json!({ "checked": expected.len(), "mismatches": mismatches }))
}
