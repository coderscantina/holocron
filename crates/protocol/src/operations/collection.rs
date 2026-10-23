//! Collection health for operators: availability, outcome counts, inbox backlog and freshness.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;

crate::domain! {
    pub trait CollectionHandlers;
    commands {}
    queries {
        /// Collection availability, per-route outcomes, inbox depth and age, and the analytical
        /// watermark. Install admins only; others get `forbidden`.
        "collection.status" => collection_status (Person): Empty => CollectionStatus;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CollectionStatus {
    pub availability: Availability,
    /// Counts below are kept since this instant, Unix milliseconds, across restarts. They are
    /// saved every 10 seconds and at a clean stop, so a crash loses at most the counts of the
    /// last 10 seconds.
    pub since: i64,
    /// `test` when a test clock drives receive time (decision 0006).
    pub clock: ClockMode,
    /// Accepted records waiting for transfer into analytics.
    pub inbox_records: i64,
    pub inbox_bytes: i64,
    /// Age of the oldest waiting record: how far analytics lags behind acknowledgement.
    pub processing_lag_ms: i64,
    /// Last applied batch and the last inbox sequence it contains.
    pub watermark_batch: i64,
    pub watermark_sequence: i64,
    pub disk_free_bytes: i64,
    /// Below this much free space collection answers 503.
    pub disk_reserve_bytes: i64,
    pub outcomes: Vec<OutcomeCount>,
    pub counters: Vec<CounterCount>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Collection acknowledges events.
    Available,
    /// The admission budget refused requests in the last minute (HTTP 429).
    Busy,
    /// Durable storage refused requests in the last minute, or free disk is below the reserve
    /// (HTTP 503).
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    System,
    Test,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRoute {
    Send,
    Batch,
    Link,
    Pixel,
    /// `/api/record`: replay parts and heatmap payloads, counted per request.
    Record,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionOutcome {
    /// Durably stored, keyless events included.
    Accepted,
    /// Same client `eventId` within 48 hours; acknowledged, not stored.
    Duplicate,
    /// Acknowledged by policy, not stored.
    Suppressed,
    /// Not stored; the client saw an error status. The unchanged tracker does not retry, so a
    /// refused event is lost.
    Refused,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReason {
    Bot,
    IpBlocked,
    OutsideRetention,
    Invalid,
    UnknownSource,
    PayloadTooLarge,
    /// Admission budget full (429).
    Busy,
    /// Durable storage unavailable (503). For links and pixels the redirect or image is still
    /// served and the hit is lost.
    Unavailable,
    /// The site's recording policy does not take this kind of recording.
    RecordingDisabled,
    /// The recording cap is reached; ordinary collection continues.
    ReplayBudget,
    /// A page the recording policy excludes.
    ExcludedPage,
    /// A collection key that is unknown, revoked, or for another site (401).
    InvalidKey,
}

/// Events per route and outcome. Batch counts items, not requests.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct OutcomeCount {
    pub route: CollectionRoute,
    pub outcome: CollectionOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<OutcomeReason>,
    pub count: i64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum CollectionCounter {
    /// A duplicate `eventId` arrived with a different payload.
    DedupConflict,
    /// Stored events more than 5 minutes ahead of receive time.
    EventTimeAhead,
    /// Stored events whose identity day key was already destroyed.
    Keyless,
    /// `consent` values ignored because the site has no persistent identity or the call had
    /// no `id`.
    ConsentIgnored,
    /// Properties beyond the count, depth or key-length limits.
    PropertyDropped,
    /// Numbers outside the integer or decimal range, stored as strings.
    PropertyCoerced,
    /// Revenue with an unknown currency, a non-numeric amount, or an overflow.
    RevenueInvalid,
    /// Site summaries the background step computed and stored (decision 0015).
    SummaryRuns,
    /// Sites due for a summary that waited for a later round because a report waited for a
    /// worker or transfer lagged behind.
    SummarySkips,
    /// Site summaries whose read hit the query timeout; the site is retried next round.
    SummaryTimeouts,
    /// Milliseconds the summary step spent on sites, timed-out reads included.
    SummaryRunMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CounterCount {
    pub counter: CollectionCounter,
    pub count: i64,
}
