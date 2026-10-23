//! `site_summaries` and `site_findings` in control storage (migration 0012), and the goals and
//! board funnels the finding rules read.

use holocron_protocol::exploration::{
    ExplorationState, GoalDefinition, GoalRef, Projection, ReportDefinition,
};
use holocron_protocol::ids::{CardId, GoalId, SiteId};
use holocron_protocol::limits::{FINDING_GOALS_MAX, METRIC_VERSION, SUMMARY_STALE_MS};
use holocron_protocol::operations::summaries::{Finding, FindingList, MeasurePace, Summary};
use holocron_protocol::retention::Retention;
use sqlx::{SqliteConnection, SqlitePool};

use super::findings::BoardFunnel;

/// A site the step may summarize.
pub struct Site {
    pub id: SiteId,
    pub domain: String,
    /// The current session policy's timezone.
    pub timezone: String,
    /// It has a summary row.
    pub summarized: bool,
    /// Its effective event retention (0016).
    pub retention: Retention,
}

/// Every site without a pending erasure or reset, with its current timezone.
pub async fn sites(pool: &SqlitePool) -> sqlx::Result<Vec<Site>> {
    let rows: Vec<(String, String, String, bool, String)> = sqlx::query_as(
        "SELECT s.id, s.domain,
                (SELECT p.timezone FROM session_policies p WHERE p.site_id = s.id
                 ORDER BY p.version DESC LIMIT 1),
                EXISTS (SELECT 1 FROM site_summaries m WHERE m.site_id = s.id),
                coalesce(s.event_retention,
                    (SELECT event_retention FROM installation_settings WHERE id = 1))
         FROM sites s
         WHERE NOT EXISTS (SELECT 1 FROM maintenance_jobs j
                           WHERE j.kind IN ('erase_site', 'reset_site')
                             AND j.state IN ('queued', 'running')
                             AND json_extract(j.args, '$.site') = s.id)
         ORDER BY s.id",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(id, domain, timezone, summarized, retention)| {
            Ok(Site {
                id: SiteId::new(id),
                domain,
                timezone,
                summarized,
                retention: crate::retention::parse(&retention)?,
            })
        })
        .collect()
}

/// Replaces the site's summary and its findings in one transaction. Writes nothing and
/// returns `false` when the site is gone, or an erasure or reset of it is pending or was
/// queued at or after `started_at`, the start of the oldest computation written: the summary
/// or a finding may then show erased data.
pub async fn write(
    pool: &SqlitePool,
    summary: &Summary,
    findings: &[Finding],
    started_at: i64,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let site = summary.site.as_str();
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sites WHERE id = ?1)
            AND NOT EXISTS (SELECT 1 FROM maintenance_jobs
                            WHERE kind IN ('erase_site', 'reset_site')
                              AND json_extract(args, '$.site') = ?1
                              AND (state IN ('queued', 'running') OR created_at >= ?2))",
    )
    .bind(site)
    .bind(started_at)
    .fetch_one(&mut *tx)
    .await?;
    if !allowed {
        return Ok(false);
    }
    let (pv, vd) = (&summary.pageviews, &summary.visitor_days);
    let basis = [&pv.projection, &vd.projection]
        .into_iter()
        .find_map(|p| match p {
            Some(Projection::Estimate { basis_days, .. }) => Some(i64::from(*basis_days)),
            _ => None,
        });
    let [pv_value, pv_low, pv_high] = band(&pv.projection);
    let [vd_value, vd_low, vd_high] = band(&vd.projection);
    sqlx::query(
        "INSERT OR REPLACE INTO site_summaries (
            site_id, computed_at, watermark_batch, watermark_sequence, metric_version, timezone,
            pageviews_today, pageviews_typical, pageviews_pace,
            pageviews_projection, pageviews_projection_low, pageviews_projection_high,
            visitor_days_today, visitor_days_typical, visitor_days_pace,
            visitor_days_projection, visitor_days_projection_low, visitor_days_projection_high,
            projection_basis_days, hourly_today, hourly_typical, insufficient_history,
            finding_count)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(site)
    .bind(summary.computed_at)
    .bind(summary.watermark_batch)
    .bind(summary.watermark_sequence)
    .bind(summary.metric_version)
    .bind(&summary.timezone)
    .bind(pv.today)
    .bind(pv.typical)
    .bind(pv.relative_change)
    .bind(pv_value)
    .bind(pv_low)
    .bind(pv_high)
    .bind(vd.today)
    .bind(vd.typical)
    .bind(vd.relative_change)
    .bind(vd_value)
    .bind(vd_low)
    .bind(vd_high)
    .bind(basis)
    .bind(serde_json::to_string(&summary.hourly_today).expect("numbers serialize"))
    .bind(serde_json::to_string(&summary.hourly_typical).expect("numbers serialize"))
    .bind(summary.insufficient_history)
    .bind(summary.finding_count)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM site_findings WHERE site_id = ?")
        .bind(site)
        .execute(&mut *tx)
        .await?;
    for finding in findings {
        let measured = serde_json::to_value(&finding.values).expect("values serialize");
        sqlx::query(
            "INSERT INTO site_findings (site_id, rank, rule_id, rule_version, kind, measured, state,
                                        window_start, share, computed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(site)
        .bind(finding.rank)
        .bind(tag(&finding.rule))
        .bind(finding.rule_version)
        .bind(measured["kind"].as_str())
        .bind(measured.to_string())
        .bind(serde_json::to_string(&finding.state).expect("states serialize"))
        .bind(finding.window_start)
        .bind(finding.share)
        .bind(finding.computed_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// The serialized name of a unit enum variant.
fn tag(value: &impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("a unit variant serializes as its name"),
    }
}

/// Removes a site's summary and findings. The erasure job calls it for a deleted or reset site.
pub async fn forget(pool: &SqlitePool, site: &str) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM site_summaries WHERE site_id = ?")
        .bind(site)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM site_findings WHERE site_id = ?")
        .bind(site)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// The stored summaries of `sites` with `stale` as of `now_ms`, in the order of `sites`. Sites
/// without a row are left out.
pub async fn read(
    conn: &mut SqliteConnection,
    sites: &[SiteId],
    now_ms: i64,
) -> sqlx::Result<Vec<Summary>> {
    let ids = serde_json::to_string(&sites).expect("IDs serialize");
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT * FROM site_summaries WHERE site_id IN (SELECT value FROM json_each(?))",
    )
    .bind(ids)
    .fetch_all(conn)
    .await?;
    let mut summaries: Vec<Summary> = rows
        .into_iter()
        .map(|row| row.summary(now_ms))
        .collect::<Result<_, _>>()?;
    summaries.sort_by_key(|s| sites.iter().position(|site| *site == s.site));
    Ok(summaries)
}

/// The site's first `limit` findings with the summary they belong to, `stale` as of `now_ms`.
pub async fn findings(
    conn: &mut SqliteConnection,
    site: &SiteId,
    limit: u32,
    now_ms: i64,
) -> sqlx::Result<FindingList> {
    type Stamp = (Option<i64>, Option<i64>, Option<i64>, Option<u32>);
    let stamp: Option<Stamp> = sqlx::query_as(
        "SELECT computed_at, watermark_batch, watermark_sequence, metric_version
         FROM site_summaries WHERE site_id = ?",
    )
    .bind(site.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let rows: Vec<FindingRow> = sqlx::query_as(
        "SELECT rank, rule_id, rule_version, measured, state, window_start, share, computed_at
         FROM site_findings WHERE site_id = ? ORDER BY rank LIMIT ?",
    )
    .bind(site.as_str())
    .bind(limit)
    .fetch_all(&mut *conn)
    .await?;
    let (computed_at, watermark_batch, watermark_sequence, metric_version) =
        stamp.unwrap_or_default();
    Ok(FindingList {
        site: site.clone(),
        computed_at,
        watermark_batch,
        watermark_sequence,
        stale: match (computed_at, metric_version) {
            (Some(at), Some(version)) => stale(at, version, now_ms),
            _ => true,
        },
        findings: rows
            .into_iter()
            .map(FindingRow::finding)
            .collect::<Result<_, _>>()?,
    })
}

/// The site's active goals, oldest first, at their current versions.
pub async fn goals(pool: &SqlitePool, site: &SiteId) -> anyhow::Result<Vec<GoalDefinition>> {
    let rows: Vec<(String, u32, String)> = sqlx::query_as(
        "SELECT g.id, g.version, v.definition FROM goals g
         JOIN goal_versions v ON v.goal_id = g.id AND v.version = g.version
         WHERE g.site_id = ? AND g.archived_at IS NULL
         ORDER BY g.created_at, g.id LIMIT ?",
    )
    .bind(site.as_str())
    .bind(FINDING_GOALS_MAX as i64)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(id, version, definition)| {
            let mut goal: GoalDefinition = serde_json::from_str(&definition)?;
            goal.goal_ref = Some(GoalRef {
                id: GoalId::new(id),
                version,
            });
            Ok(goal)
        })
        .collect()
}

/// The funnels pinned to boards for the site, boards by creation, cards by position. Cards
/// whose state this build no longer reads are left out.
pub async fn funnels(pool: &SqlitePool, site: &SiteId) -> sqlx::Result<Vec<BoardFunnel>> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT c.id, c.title, c.state FROM board_cards c JOIN boards b ON b.id = c.board_id
         WHERE c.site_id = ? AND json_extract(c.state, '$.report.kind') = 'funnel'
         ORDER BY b.created_at, b.id, c.position",
    )
    .bind(site.as_str())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(card, title, state)| {
            let state: ExplorationState = serde_json::from_str(&state).ok()?;
            match state.report? {
                ReportDefinition::Funnel(funnel) => Some(BoardFunnel {
                    card: CardId::new(card),
                    title,
                    funnel,
                    filter: state.filter,
                }),
                _ => None,
            }
        })
        .collect())
}

fn stale(computed_at: i64, metric_version: u32, now_ms: i64) -> bool {
    now_ms - computed_at > SUMMARY_STALE_MS || metric_version != METRIC_VERSION
}

#[derive(sqlx::FromRow)]
struct FindingRow {
    rank: u32,
    rule_id: String,
    rule_version: u32,
    measured: String,
    state: String,
    window_start: i64,
    share: f64,
    computed_at: i64,
}

impl FindingRow {
    fn finding(self) -> sqlx::Result<Finding> {
        let decode = |e: serde_json::Error| sqlx::Error::Decode(e.into());
        Ok(Finding {
            rank: self.rank,
            rule: serde_json::from_value(serde_json::Value::String(self.rule_id))
                .map_err(decode)?,
            rule_version: self.rule_version,
            computed_at: self.computed_at,
            window_start: self.window_start,
            share: self.share,
            state: serde_json::from_str(&self.state).map_err(decode)?,
            values: serde_json::from_str(&self.measured).map_err(decode)?,
        })
    }
}

fn band(projection: &Option<Projection>) -> [Option<f64>; 3] {
    match projection {
        Some(Projection::Estimate {
            value, low, high, ..
        }) => [Some(*value), Some(*low), Some(*high)],
        _ => [None; 3],
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    site_id: String,
    computed_at: i64,
    watermark_batch: i64,
    watermark_sequence: i64,
    metric_version: u32,
    timezone: String,
    pageviews_today: i64,
    pageviews_typical: Option<i64>,
    pageviews_pace: Option<f64>,
    pageviews_projection: Option<f64>,
    pageviews_projection_low: Option<f64>,
    pageviews_projection_high: Option<f64>,
    visitor_days_today: i64,
    visitor_days_typical: Option<i64>,
    visitor_days_pace: Option<f64>,
    visitor_days_projection: Option<f64>,
    visitor_days_projection_low: Option<f64>,
    visitor_days_projection_high: Option<f64>,
    projection_basis_days: Option<u32>,
    hourly_today: String,
    hourly_typical: String,
    insufficient_history: bool,
    finding_count: u32,
}

impl Row {
    fn summary(self, now_ms: i64) -> sqlx::Result<Summary> {
        let estimate = |value: Option<f64>, low: Option<f64>, high: Option<f64>| {
            Some(Projection::Estimate {
                value: value?,
                low: low?,
                high: high?,
                basis_days: self.projection_basis_days?,
            })
        };
        fn json<T: serde::de::DeserializeOwned>(text: &str) -> sqlx::Result<T> {
            serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(e.into()))
        }
        Ok(Summary {
            stale: stale(self.computed_at, self.metric_version, now_ms),
            pageviews: MeasurePace {
                today: self.pageviews_today,
                typical: self.pageviews_typical,
                relative_change: self.pageviews_pace,
                projection: estimate(
                    self.pageviews_projection,
                    self.pageviews_projection_low,
                    self.pageviews_projection_high,
                ),
            },
            visitor_days: MeasurePace {
                today: self.visitor_days_today,
                typical: self.visitor_days_typical,
                relative_change: self.visitor_days_pace,
                projection: estimate(
                    self.visitor_days_projection,
                    self.visitor_days_projection_low,
                    self.visitor_days_projection_high,
                ),
            },
            hourly_today: json(&self.hourly_today)?,
            hourly_typical: json(&self.hourly_typical)?,
            site: SiteId::new(self.site_id),
            computed_at: self.computed_at,
            watermark_batch: self.watermark_batch,
            watermark_sequence: self.watermark_sequence,
            metric_version: self.metric_version,
            timezone: self.timezone,
            insufficient_history: self.insufficient_history,
            finding_count: self.finding_count,
        })
    }
}
