//! Exploration of one site: typed explorations and their exports, the field catalog,
//! realtime, and the site's saved explorations, segments and annotations.
//!
//! Exploration states travel as JSON text in the `state` argument, so a query string carries
//! the same document as a URL, a saved exploration and a board card (decision 0004).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::exploration::{Cell, Coverage, ExplorationState, Group, PropertyScope};
use crate::ids::{AnnotationId, ExplorationId, SegmentId, SiteId};
use crate::operations::analytics::Freshness;

crate::domain! {
    pub trait ExplorationsHandlers;
    commands {
        /// Saves the complete state under a name, or replaces the state and name of one of
        /// the caller's saved explorations. Saved explorations are personal. Needs `site.read`.
        "explorations.save" => explorations_save (Person, key): ExplorationsSaveArgs => SavedExplorationResult;
        /// Deletes one of the caller's saved explorations. Needs `site.read`.
        "explorations.delete" => explorations_delete (Person, key): ExplorationsGetArgs => Deleted;
        /// Saves a named filter group for everyone who can read the site. Names are unique per
        /// site after case folding. Needs `site.definitions.write`.
        "segments.save" => segments_save (Person, key): SegmentsSaveArgs => SegmentResult;
        "segments.delete" => segments_delete (Person, key): SegmentsDeleteArgs => Deleted;
        /// Adds a note at an instant of the site's charts. Needs `site.definitions.write`.
        "annotations.create" => annotations_create (Person, key): AnnotationsCreateArgs => AnnotationResult;
        "annotations.update" => annotations_update (Person, key): AnnotationsUpdateArgs => AnnotationResult;
        "annotations.delete" => annotations_delete (Person, key): AnnotationsDeleteArgs => Deleted;
    }
    queries {
        /// Computes an exploration state and optional panels (other cuts of the same rows and
        /// range) in one analytical snapshot. Needs `site.read`. Refusals name the field:
        /// `invalid_arguments`, `duplicate_predicate_id`, `filter_too_deep`,
        /// `too_many_conditions`, `page_out_of_range`, `report_missing`.
        "analytics.explore" => analytics_explore (Person, key, share_read): ExploreArgs => crate::exploration::ExploreResult;
        /// Answers a report query for agents (decision 0022): named blocks, each a cut or a
        /// report on one site, as typed tables from one analytical snapshot. Send it as the JSON
        /// body of `POST /api/queries/analytics.report`. Needs `site.read` on every block's
        /// site. Refusals name the path, as `blocks.2.filter.f3`: `invalid_arguments`,
        /// `report_too_large` (over 16 blocks or 200 sub-queries, before anything runs),
        /// `report_field_unsupported` (session lists).
        "analytics.report" => analytics_report (Person, key): crate::report_query::ReportQuery => crate::report_query::ReportQueryResult;
        /// Every row of an exploration (up to 10,000) as a file, with the state, resolved range
        /// and versions it was computed from. Needs `site.export`.
        "analytics.export" => analytics_export (Person, key, share_export): ExportArgs => ExportFile;
        /// Custom property keys seen on the site, with their value types. Needs `site.read`.
        "fields.catalog" => fields_catalog (Person, key): FieldsCatalogArgs => FieldCatalog;
        /// The most frequent values of one field over the last 30 days. Needs `site.read`. A
        /// share viewer gets values of granted fields only, inside the share's pinned filter and
        /// range limit, with `events` 0 unless the share grants the `events` measure.
        "fields.values" => fields_values (Person, key, share_read): FieldsValuesArgs => FieldValues;
        /// Active visitors, the last 30 minutes and recent events, with the cursor to follow
        /// changes on `GET /api/subscribe`. Needs `site.read`.
        "realtime.snapshot" => realtime_snapshot (Person, key): RealtimeArgs => RealtimeSnapshot;
        /// One session: its fields, its pageviews and custom events in order (up to 500) and
        /// the latest value of each identified session property. Needs `site.read`; an unknown
        /// session is `not_found`.
        "sessions.get" => sessions_get (Person, key): SessionsGetArgs => SessionDetail;
        /// The latest sessions starting in the last `hours`, with their pageviews and custom
        /// events by time, for a timeline. Needs `site.read`.
        "sessions.timeline" => sessions_timeline (Person, key): SessionsTimelineArgs => SessionTimeline;
        /// The caller's saved explorations of the site, newest first. Needs `site.read`.
        "explorations.list" => explorations_list (Person, key): SiteArgs => SavedExplorationList;
        "explorations.get" => explorations_get (Person, key): ExplorationsGetArgs => SavedExplorationResult;
        /// The site's segments by name. Needs `site.read`.
        "segments.list" => segments_list (Person, key): SiteArgs => SegmentList;
        /// Annotations with `start <= at < end`, oldest first. Needs `site.read`.
        "annotations.list" => annotations_list (Person, key): AnnotationsListArgs => AnnotationList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SiteArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExploreArgs {
    /// JSON text of an `ExplorationState`.
    pub state: String,
    /// JSON text of up to 8 `Panel`s.
    #[serde(default)]
    pub panels: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Csv,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExportArgs {
    /// JSON text of an `ExplorationState`.
    pub state: String,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExportFile {
    pub filename: String,
    pub content_type: String,
    pub content: String,
    pub row_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldsCatalogArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldCatalog {
    /// Up to 500 keys, most used first.
    pub properties: Vec<PropertyKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PropertyKey {
    pub scope: PropertyScope,
    pub key: String,
    /// Value types stored under the key; one key can carry several (0002).
    pub types: Vec<PropertyType>,
    /// Rows stored under the key.
    pub rows: i64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PropertyType {
    Null,
    Bool,
    Integer,
    Decimal,
    Date,
    String,
    Array,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldsValuesArgs {
    pub site: SiteId,
    /// JSON text of a `Field`.
    pub field: String,
    /// Only values starting with this text, compared case-insensitively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldValues {
    /// Up to 20 values, most frequent first.
    pub values: Vec<FieldValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct FieldValue {
    pub value: Cell,
    pub events: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RealtimeArgs {
    pub site: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RealtimeSnapshot {
    /// Application clock at the read, Unix milliseconds.
    pub now: i64,
    /// Distinct daily pseudonyms with activity in `[now − 5 min, now]` (0003 Realtime).
    pub active: i64,
    /// 30 one-minute buckets, oldest first; the last is the current partial minute.
    pub series: Vec<RealtimeBucket>,
    /// The 20 latest pageviews and custom events by activity time.
    pub recent: Vec<RecentEvent>,
    /// Where the `active` visitors are, by their latest event in the window. Visitors without a
    /// country have no location, so these add up to `active` at most.
    pub active_locations: Vec<ActiveLocation>,
    pub identity_coverage: Coverage,
    pub freshness: Freshness,
    /// Accepted records not yet visible to analytics, and the age of the oldest.
    pub processing: Processing,
    /// Pass to `GET /api/subscribe?topic=…&cursor=…` to hear about newer data.
    pub live_topic: String,
    pub live_cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RealtimeBucket {
    pub start: i64,
    pub pageviews: i64,
    pub visitors: i64,
}

/// The active visitors in one 0.1° cell, or at a country's centroid when their coordinates
/// are unknown (decision 0017).
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ActiveLocation {
    /// Degrees, rounded to 0.1°.
    pub lat: f64,
    pub lng: f64,
    pub country: String,
    /// Absent at a country's centroid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub city: Option<String>,
    pub visitors: i64,
    /// The path of the latest event in the cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct RecentEvent {
    /// Activity time: the earlier of event time and receive time.
    pub at: i64,
    pub kind: RecentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub event_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub country: Option<String>,
    /// Absent when the location is the country's centroid, as in [`ActiveLocation`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub city: Option<String>,
    /// Degrees, rounded to 0.1°; the country's centroid without coordinates, absent without a
    /// country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lng: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub browser: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionsGetArgs {
    pub site: SiteId,
    /// The session ID from the session list report.
    pub session: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionsTimelineArgs {
    pub site: SiteId,
    /// 1 to `SESSION_TIMELINE_HOURS_MAX`.
    pub hours: u32,
}

/// Sessions starting in `[from, to)`, the latest `SESSION_TIMELINE_SESSIONS_MAX` of them.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionTimeline {
    /// The window, Unix milliseconds; `to` is the application clock at the read.
    pub from: i64,
    pub to: i64,
    /// Sessions starting in the window, also those not in `sessions`.
    pub total: i64,
    /// Median observed span of all those sessions with a pageview, in milliseconds, as the
    /// `span_median` measure reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub median_span: Option<i64>,
    /// Latest start first.
    pub sessions: Vec<TimelineSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TimelineSession {
    /// The session ID, as `sessions.get` takes it.
    pub session: i64,
    pub person: bool,
    /// First and last member event, Unix milliseconds.
    pub start: i64,
    pub end: i64,
    pub pageviews: i64,
    pub custom_events: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub browser: Option<String>,
    /// The first `SESSION_TIMELINE_EVENTS_MAX` pageviews and custom events by event time,
    /// without titles.
    pub events: Vec<SessionEvent>,
    /// The session has more events than `events` carries.
    pub truncated: bool,
}

/// One session in detail. Fields are the session's stored values: the first member event's,
/// then the touch's (0004 Field scope).
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionDetail {
    pub session: i64,
    /// A consented person's session rather than a daily visitor's.
    pub person: bool,
    /// First and last member event, Unix milliseconds.
    pub start: i64,
    pub end: i64,
    pub pageviews: i64,
    pub custom_events: i64,
    pub fields: SessionFields,
    /// Pageviews and custom events by event time.
    pub events: Vec<SessionEvent>,
    /// The session has more events than `events` carries.
    pub truncated: bool,
    /// The latest value of each identified session property, by key.
    pub properties: Vec<SessionProperty>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub referrer_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub referrer_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub utm_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub utm_medium: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub utm_campaign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub browser: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub screen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionEvent {
    /// Event time, Unix milliseconds.
    pub at: i64,
    pub kind: RecentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub event_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SessionProperty {
    pub key: String,
    pub value: Cell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind {
    Pageview,
    CustomEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Processing {
    pub inbox_records: i64,
    pub inbox_age_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedExploration {
    pub id: ExplorationId,
    pub site: SiteId,
    pub name: String,
    pub state: ExplorationState,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedExplorationList {
    pub explorations: Vec<SavedExploration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SavedExplorationResult {
    pub exploration: SavedExploration,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExplorationsGetArgs {
    pub site: SiteId,
    pub exploration: ExplorationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ExplorationsSaveArgs {
    pub site: SiteId,
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// The complete state. Its `site` must be `site`.
    pub state: ExplorationState,
    /// Replaces this saved exploration instead of creating one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exploration: Option<ExplorationId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Deleted {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Segment {
    pub id: SegmentId,
    pub site: SiteId,
    pub name: String,
    pub filter: Group,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SegmentList {
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SegmentResult {
    pub segment: Segment,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SegmentsSaveArgs {
    pub site: SiteId,
    pub name: String,
    pub filter: Group,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SegmentsDeleteArgs {
    pub site: SiteId,
    pub segment: SegmentId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Annotation {
    pub id: AnnotationId,
    pub site: SiteId,
    /// Unix milliseconds.
    pub at: i64,
    pub text: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationList {
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationResult {
    pub annotation: Annotation,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationsListArgs {
    pub site: SiteId,
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationsCreateArgs {
    pub site: SiteId,
    pub at: i64,
    /// 1 to 500 characters after trimming.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationsUpdateArgs {
    pub site: SiteId,
    pub annotation: AnnotationId,
    pub at: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AnnotationsDeleteArgs {
    pub site: SiteId,
    pub annotation: AnnotationId,
}

/// One message of `GET /api/subscribe` (architecture patterns, Topics, cursors and
/// subscriptions), sent as the `data` of a server-sent event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveFrame {
    /// The stream is open; `head` is the topic's current position.
    Subscribed { head: String, cursor: String },
    /// Newer data exists; `cursor` is the position after the last event.
    Events {
        cursor: String,
        events: Vec<LiveEvent>,
    },
    /// Subscribe again from `cursor`; nothing after it was delivered.
    Resync {
        cursor: String,
        reason: ResyncReason,
    },
    /// The cursor is not on this topic's history: read a fresh snapshot.
    Expired { head: String },
    /// The caller lost access to the topic (membership, role, key or share changed, R-SUB).
    /// The stream ends; subscribing again answers like a missing resource.
    AccessRevoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct LiveEvent {
    pub seq: i64,
    pub kind: LiveEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LiveEventKind {
    /// The analytical watermark moved: accepted records became visible.
    Watermark,
    /// The site's summary was computed again; `seq` is the batch it was computed at. Read
    /// `summaries.forSites` again.
    Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    SlowSubscriber,
    Stalled,
    /// Access could not be checked just now; subscribe again from `cursor`.
    Unavailable,
}
