//! Site summaries (decision 0015 Where it runs): a background task keeps one row per site with
//! today's pace against a typical weekday, so headers, menus and the workspace page read one
//! SQLite row instead of starting an analytical computation.
//!
//! A site is due when it has no summary from this process yet, or when the analytical
//! watermark moved past its last summary and [`SUMMARY_CADENCE_MS`] passed since. A round
//! summarizes due sites one at a time through the report lane, and only while a report would
//! start at once and transfer keeps up (as compaction yields, 0014); otherwise the rest of the
//! round waits. A read that hits the lane's query timeout is logged and retried next round. The
//! schedule lives in memory, so every site is summarized again after a start, a restored backup
//! included.
//!
//! With each summary the step runs the finding rules ([`findings`]) and replaces the site's
//! findings with the row, in the same transaction.
//!
//! When a row changes, the site's `analytics:<site>` stream sends a `summary` event
//! ([`Summaries::subscribe`]), so open dashboards read `summaries.forSites` and
//! `summaries.findings` again instead of polling.

mod findings;
mod pace;
mod store;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use chrono::NaiveDate;
use holocron_analytics::explore::{self, Context};
use holocron_protocol::access::Capability;
use holocron_protocol::envelope::Topic;
use holocron_protocol::errors::{ErrorCode, FieldErrorCode};
use holocron_protocol::ids::SiteId;
use holocron_protocol::limits::{FINDINGS_MAX, SUMMARY_CADENCE_MS, SUMMARY_SITES_MAX};
use holocron_protocol::operations::collection::CollectionCounter;
use holocron_protocol::operations::summaries::{
    FindingList, SummariesFindingsArgs, SummariesForSitesArgs, SummariesHandlers, Summary,
    SummaryList,
};
use holocron_protocol::registry::Snapshot;
use holocron_storage::{Lane, ReadError, Watermark};
use tokio::sync::{broadcast, watch};

pub use pace::state;
pub use store::forget;

use crate::context::QueryCtx;
use crate::error::{ApiError, FieldErrors};
use crate::maintenance::compaction::transfer_current;
use crate::sites::policy::analytics_id;
use crate::state::AppState;

/// Time between rounds.
const ROUND: Duration = Duration::from_secs(60);

/// Notifies open streams that a site's summary changed.
pub struct Summaries {
    changed: broadcast::Sender<(SiteId, i64)>,
}

impl Default for Summaries {
    fn default() -> Self {
        Self {
            changed: broadcast::channel(256).0,
        }
    }
}

impl Summaries {
    /// Each site whose summary was written, with the batch it was computed at. A receiver that
    /// lags behind misses names and should treat that as a change of its site.
    pub fn subscribe(&self) -> broadcast::Receiver<(SiteId, i64)> {
        self.changed.subscribe()
    }
}

/// Runs rounds until `stop` changes. The first round starts one [`ROUND`] after start.
pub async fn follow(state: AppState, mut stop: watch::Receiver<bool>) {
    let mut schedule = Schedule::default();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(ROUND) => {}
            _ = stop.changed() => return,
        }
        if let Err(error) = round(&state, &mut schedule, &stop).await {
            tracing::error!(error = %error, "site summaries failed; retrying next round");
        }
    }
}

/// When each site was last summarized in this process, and what its rules last found.
#[derive(Default)]
pub struct Schedule {
    last: HashMap<SiteId, Ran>,
    kept: HashMap<SiteId, findings::Kept>,
}

#[derive(Clone, Copy)]
struct Ran {
    at_ms: i64,
    watermark: Watermark,
    /// A row was written. A failed run waits out the cadence like a written one.
    wrote: bool,
}

impl Schedule {
    fn due(&self, site: &store::Site, watermark: Watermark, now_ms: i64) -> bool {
        match self.last.get(&site.id) {
            None => true,
            // The row went: the site was reset.
            Some(ran) if ran.wrote && !site.summarized => true,
            Some(ran) => watermark > ran.watermark && now_ms - ran.at_ms >= SUMMARY_CADENCE_MS,
        }
    }
}

/// What one round did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Round {
    /// Sites summarized, in order.
    pub written: Vec<SiteId>,
    /// Due sites left for a later round because a report waited or transfer lagged.
    pub skipped: usize,
    /// Sites whose read timed out; they are due again next round.
    pub timed_out: Vec<SiteId>,
    /// Sites whose summary failed for another reason; logged, retried after the cadence.
    pub failed: usize,
}

/// Summarizes every due site, never-summarized and longest-waiting first, one at a time.
pub async fn round(
    state: &AppState,
    schedule: &mut Schedule,
    stop: &watch::Receiver<bool>,
) -> anyhow::Result<Round> {
    let storage = state.collector.storage();
    let metrics = state.collector.metrics();
    let clock = state.collector.clock();
    let watermark = *storage.watermark().borrow();
    let now = clock.now_ms();
    let sites = store::sites(&state.db.reader).await?;
    let listed: HashSet<&SiteId> = sites.iter().map(|site| &site.id).collect();
    schedule.last.retain(|id, _| listed.contains(id));
    schedule.kept.retain(|id, _| listed.contains(id));
    let mut due: Vec<_> = sites
        .into_iter()
        .filter(|site| schedule.due(site, watermark, now))
        .collect();
    due.sort_by_key(|site| schedule.last.get(&site.id).map(|ran| ran.at_ms));

    let mut round = Round::default();
    for (index, site) in due.iter().enumerate() {
        if *stop.borrow() {
            break;
        }
        if !storage.reports_idle() || !transfer_current(storage).await? {
            round.skipped = due.len() - index;
            break;
        }
        let started = Instant::now();
        let started_at = clock.now_ms();
        let kept = schedule.kept.entry(site.id.clone()).or_default();
        let outcome = summarize(state, site, started_at, kept).await;
        metrics.count(
            CollectionCounter::SummaryRunMs,
            started.elapsed().as_millis() as u64,
        );
        let ran = |wrote, watermark| Ran {
            at_ms: started_at,
            watermark,
            wrote,
        };
        let summary = match outcome {
            Ok(summary) => summary,
            Err(error) => match error.downcast_ref::<ReadError>() {
                Some(ReadError::Timeout) => {
                    tracing::warn!(site = %site.id, "site summary timed out; retrying next round");
                    metrics.count(CollectionCounter::SummaryTimeouts, 1);
                    round.timed_out.push(site.id.clone());
                    continue;
                }
                Some(ReadError::Busy) => {
                    round.skipped = due.len() - index;
                    break;
                }
                _ => {
                    tracing::error!(site = %site.id, error = %error, "site summary failed");
                    schedule.last.insert(site.id.clone(), ran(false, watermark));
                    round.failed += 1;
                    continue;
                }
            },
        };
        // `None`: the site went or is being erased; the next round no longer lists it.
        if let Some(summary) = summary {
            let at = Watermark {
                batch: summary.watermark_batch,
                sequence: summary.watermark_sequence,
            };
            schedule.last.insert(site.id.clone(), ran(true, at));
            metrics.count(CollectionCounter::SummaryRuns, 1);
            // No receiver means no open stream; nothing to tell.
            let _ = state.summaries.changed.send((site.id.clone(), at.batch));
            round.written.push(site.id.clone());
        }
    }
    metrics.count(CollectionCounter::SummarySkips, round.skipped as u64);
    Ok(round)
}

/// Computes and stores one site's summary and findings; `None` when the site may no longer be
/// written.
async fn summarize(
    state: &AppState,
    site: &store::Site,
    now_ms: i64,
    kept: &mut findings::Kept,
) -> anyhow::Result<Option<Summary>> {
    let query = pace::query(&site.id, &site.timezone)?;
    let analytics = analytics_id(site.id.as_str())?;
    let hostnames = vec![site.domain.clone()];
    let reading = hostnames.clone();
    let retention = site.retention;
    let result = state
        .collector
        .storage()
        .read(Lane::Report, move |snapshot| {
            let context = Context {
                site: analytics,
                now_ms,
                hostnames: &reading,
                retention,
            };
            explore::explore(snapshot, &query, &context)
        })
        .await?
        .value
        .map_err(|refusal| anyhow::anyhow!("summary refused: {}", refusal.message))?;
    let mut summary = pace::summary(&site.id, &site.timezone, &result, now_ms)?;

    // Without its row the site was reset: nothing found before may stay.
    if !site.summarized {
        kept.clear();
    }
    let (today, hour) =
        findings::Site::local(&site.timezone, now_ms).context("unknown timezone")?;
    let baseline_days = result
        .resolved
        .compare_days
        .iter()
        .flatten()
        .map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d"))
        .collect::<Result<_, _>>()?;
    let reader = &state.db.reader;
    let context = findings::Site {
        id: site.id.clone(),
        analytics,
        timezone: site.timezone.clone(),
        hostnames,
        retention,
        now_ms,
        today,
        hour,
        baseline_days,
        goals: store::goals(reader, &site.id).await?,
        funnels: store::funnels(reader, &site.id).await?,
    };
    let found = findings::notice(state, context, kept).await;
    summary.finding_count = found.len() as u32;
    let oldest = found.iter().map(|f| f.computed_at).fold(now_ms, i64::min);
    Ok(store::write(&state.db.writer, &summary, &found, oldest)
        .await?
        .then_some(summary))
}

impl SummariesHandlers for AppState {
    async fn summaries_for_sites(
        &self,
        ctx: &mut QueryCtx,
        args: SummariesForSitesArgs,
    ) -> Result<Snapshot<SummaryList>, ApiError> {
        if args.sites.is_empty() {
            return Err(FieldErrors::one("sites", FieldErrorCode::Required, None));
        }
        if args.sites.len() > SUMMARY_SITES_MAX {
            return Err(FieldErrors::one(
                "sites",
                FieldErrorCode::TooMany,
                Some(SUMMARY_SITES_MAX as u32),
            ));
        }
        let mut readable: Vec<SiteId> = Vec::with_capacity(args.sites.len());
        for site in args.sites {
            if readable.contains(&site) {
                continue;
            }
            match ctx.site_scope(&site, Capability::SiteRead).await {
                Ok(_) => readable.push(site),
                // Hidden and unreadable sites are left out, exactly like missing ones.
                Err(error)
                    if matches!(error.code(), ErrorCode::NotFound | ErrorCode::Forbidden) => {}
                Err(error) => return Err(error),
            }
        }
        let now = self.collector.clock().now_ms();
        let summaries = store::read(ctx.conn(), &readable, now).await?;
        Ok(Snapshot {
            topic: Topic::installation(),
            data: SummaryList { summaries },
        })
    }

    async fn summaries_findings(
        &self,
        ctx: &mut QueryCtx,
        args: SummariesFindingsArgs,
    ) -> Result<Snapshot<FindingList>, ApiError> {
        let scope = ctx.site_scope(&args.site, Capability::SiteRead).await?;
        if args.limit == 0 || args.limit as usize > FINDINGS_MAX {
            return Err(FieldErrors::one(
                "limit",
                FieldErrorCode::InvalidValue,
                Some(FINDINGS_MAX as u32),
            ));
        }
        let now = self.collector.clock().now_ms();
        let list = store::findings(ctx.conn(), scope.site(), args.limit, now).await?;
        Ok(Snapshot {
            topic: Topic::site(scope.site()),
            data: list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use holocron_protocol::retention::Retention;

    fn site(summarized: bool) -> store::Site {
        store::Site {
            id: SiteId::new("s1"),
            domain: "example.test".into(),
            timezone: "UTC".into(),
            summarized,
            retention: Retention::OneYear,
        }
    }

    fn at(sequence: i64) -> Watermark {
        Watermark {
            batch: sequence,
            sequence,
        }
    }

    #[test]
    fn a_site_is_due_after_the_watermark_moved_and_the_cadence_passed() {
        let mut schedule = Schedule::default();
        assert!(schedule.due(&site(false), at(5), 0), "never summarized");
        schedule.last.insert(
            SiteId::new("s1"),
            Ran {
                at_ms: 0,
                watermark: at(5),
                wrote: true,
            },
        );
        let summarized = site(true);
        assert!(!schedule.due(&summarized, at(6), SUMMARY_CADENCE_MS - 1));
        assert!(!schedule.due(&summarized, at(5), SUMMARY_CADENCE_MS));
        assert!(schedule.due(&summarized, at(6), SUMMARY_CADENCE_MS));
        assert!(
            schedule.due(&site(false), at(5), 1),
            "a reset removed the row"
        );
        // A failed run leaves no row and still waits out the cadence.
        schedule.last.get_mut(&SiteId::new("s1")).unwrap().wrote = false;
        assert!(!schedule.due(&site(false), at(5), 1));
    }
}
