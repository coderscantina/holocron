//! Replay and heatmaps: the recording policy of a site, captured replays, playback pages,
//! saved replays, the installation's recording budget, and heatmap cells.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::SiteId;

crate::domain! {
    pub trait RecordingsHandlers;
    commands {
        /// Changes what the recorder captures on a site. Needs `site.configure`. Recording is off
        /// until enabled; masking is always strict. Refused with `stale_revision` when
        /// `expected_revision` is older than the site topic. An unchanged policy records nothing.
        "recordings.updatePolicy" => recordings_update_policy (Person, key): RecordingPolicyUpdateArgs => RecordingPolicyResult;
        /// Keeps a replay past the 14-day removal. Needs `site.recordings.read`. Saved replays
        /// share a 5 GiB part of the recording cap per installation; a save beyond it is refused
        /// with `saved_replay_budget_exceeded`. Saving a saved replay changes nothing.
        "recordings.save" => recordings_save (Person, key): CaptureArgs => CaptureResult;
        /// Returns a saved replay to the 14-day removal. Needs `site.recordings.read`.
        "recordings.unsave" => recordings_unsave (Person, key): CaptureArgs => CaptureResult;
    }
    queries {
        /// The recording policy of a site. Needs `site.configure`.
        "recordings.policy" => recordings_policy (Person, key): RecordingSiteArgs => RecordingPolicyResult;
        /// Recording storage of the whole installation against its cap. Needs `site.configure`
        /// on the named site.
        "recordings.budget" => recordings_budget (Person, key): RecordingSiteArgs => RecordingBudget;
        /// Captured replays received in `[start, end)`, newest first, at most `limit` per page.
        /// Needs `site.recordings.read`. A share viewer lists only replays inside the share's
        /// range limit and pinned filter; a page may then hold fewer than `limit` with `next`.
        /// With `filter`, the session match runs on the report lane with its timeout
        /// (`query_timeout`); a refused filter answers with the exploration's error codes.
        "recordings.list" => recordings_list (Person, key, share_recordings): RecordingsListArgs => RecordingList;
        /// One page of a replay's events in recording order, at most 4 MiB of event JSON. Pass
        /// `next` as `after` for the following page. Needs `site.recordings.read`. A replay
        /// outside a viewer's share is `not_found`.
        "recordings.get" => recordings_get (Person, key, share_recordings): RecordingsGetArgs => ReplayPage;
        /// Pages with heatmap interactions in `[start, end)`, most clicked first, at most 100.
        /// Needs `site.recordings.read`.
        "heatmaps.pages" => heatmaps_pages (Person, key): HeatmapPagesArgs => HeatmapPages;
        /// Click cells and scroll depth of one page and viewport class. Needs
        /// `site.recordings.read`.
        "heatmaps.cells" => heatmaps_cells (Person, key): HeatmapCellsArgs => HeatmapCells;
    }
}

/// A captured replay: one visit of the recorder's session token. Not an analytical session.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
pub struct CaptureId(String);

impl CaptureId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingSiteArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingPolicy {
    pub replay_enabled: bool,
    pub heatmap_enabled: bool,
    /// Share of page views the recorder records a replay for, 1 to 100. Default 10.
    pub replay_sample_percent: u8,
    /// Share of page views that send heatmap interactions, 1 to 100. Default 10.
    pub heatmap_sample_percent: u8,
    /// A replay stops after this many minutes of one page, 1 to 60. Default 5.
    pub max_duration_minutes: u16,
    /// CSS selectors whose elements the recorder never captures, at most 20.
    pub excluded_elements: Vec<String>,
    /// URL paths that are never recorded, `*` matches any characters, at most 50. The unchanged
    /// recorder cannot skip a page itself: the server drops what it sends from these pages.
    pub excluded_paths: Vec<String>,
    /// Counts policy changes, starting at 1. 0 when the policy was never set.
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingPolicyResult {
    pub policy: RecordingPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingPolicyUpdateArgs {
    pub site: SiteId,
    pub replay_enabled: bool,
    pub heatmap_enabled: bool,
    pub replay_sample_percent: u8,
    pub heatmap_sample_percent: u8,
    pub max_duration_minutes: u16,
    pub excluded_elements: Vec<String>,
    pub excluded_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingBudget {
    /// Compressed replay bytes stored, saved replays included.
    pub used_bytes: i64,
    /// New replay data is refused above this (default 25 GiB).
    pub cap_bytes: i64,
    pub saved_bytes: i64,
    /// Saved replays may hold at most this much of the cap (5 GiB).
    pub saved_cap_bytes: i64,
    pub captures: i64,
    /// Size of the replay database file, free pages included.
    pub file_bytes: i64,
    /// The cap refused replay data within the last minute.
    pub refusing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingsListArgs {
    pub site: SiteId,
    /// Receive time range, Unix milliseconds, `start` inclusive, `end` exclusive.
    pub start: i64,
    pub end: i64,
    /// Only replays that visited a page whose path contains this text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    /// Only replays from this device class, for example `mobile`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min_duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub saved: Option<bool>,
    /// Only replays of this analytical session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session: Option<i64>,
    /// JSON text of a `Group`: only replays whose analytical session, found as for `session`,
    /// has at least one pageview or custom event matching it. Checked like an exploration's
    /// `filter`, with the same limits (0004); a refused filter lists nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filter: Option<String>,
    /// The `next` cursor of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// 1 to 100, default 25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CaptureSummary {
    pub id: CaptureId,
    /// When the first part arrived, Unix milliseconds.
    pub received_at: i64,
    /// Span of the recorded events by the browser's clock.
    pub duration_ms: i64,
    pub events: i64,
    pub bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub entry_path: Option<String>,
    pub pages: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub browser: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub country: Option<String>,
    pub saved: bool,
    /// Later parts were refused at the recording cap.
    pub truncated: bool,
    /// The analytical session the replay belongs to under the current calculation, when
    /// one contains its start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingList {
    pub captures: Vec<CaptureSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next: Option<String>,
    /// With `filter`, on the first page: every replay the list holds across its pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub total: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CaptureArgs {
    pub site: SiteId,
    pub capture: CaptureId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct CaptureResult {
    pub capture: CaptureSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecordingsGetArgs {
    pub site: SiteId,
    pub capture: CaptureId,
    /// The `next` cursor of the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ReplayPage {
    pub capture: CaptureSummary,
    /// JSON array of rrweb events in recording order, split events restored and active
    /// content removed. Untrusted: render it only in the isolated replay frame.
    pub events: String,
    pub timeline: Vec<TimelineEntry>,
    /// Split events of this page that could not be restored because a part is missing.
    pub gaps: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next: Option<String>,
}

/// One step of the accessible event timeline. Input values and page text are never listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TimelineEntry {
    /// Browser time of the event, Unix milliseconds.
    pub at: i64,
    pub kind: TimelineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    /// A page loaded or the URL changed.
    Page,
    Click,
    Input,
    Scroll,
    Resize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapPagesArgs {
    pub site: SiteId,
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapPages {
    pub pages: Vec<HeatmapPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapPage {
    pub path: String,
    pub clicks: i64,
    /// Page visits with scroll depth.
    pub scroll_visits: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewportClass {
    /// Narrower than 768 px.
    Mobile,
    /// 768 to 1279 px.
    Tablet,
    /// 1280 px and wider.
    Desktop,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapCellsArgs {
    pub site: SiteId,
    pub path: String,
    pub start: i64,
    pub end: i64,
    pub viewport: ViewportClass,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapCells {
    /// Click cells divide the viewport width into this many columns.
    pub columns: u16,
    /// Height of one cell row in page pixels.
    pub row_height: u16,
    /// The most clicked cells, at most 4096.
    pub clicks: Vec<HeatmapCell>,
    pub total_clicks: i64,
    /// Page visits that reached each depth, from 0 to 100 percent in steps of 10.
    pub scroll: Vec<ScrollDepth>,
    pub scroll_visits: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeatmapCell {
    pub column: u16,
    pub row: u32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ScrollDepth {
    pub depth_percent: u8,
    pub visits: i64,
}
