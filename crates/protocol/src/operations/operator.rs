//! Operator status for install admins: collection health, readiness problems, disk and
//! recording budgets, recent maintenance outcomes, and the storage forecast (0008 Budgets,
//! 0014 Jobs, Compaction and Health). Everything here is counts, sizes, times, states and
//! fixed names: no event bodies, identity inputs or site data.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::operations::collection::CollectionStatus;
use crate::operations::recordings::RecordingBudget;

crate::domain! {
    pub trait OperatorHandlers;
    commands {}
    queries {
        /// Collection state, readiness problems, disk and recording budgets, the latest
        /// retention, compaction and backup outcomes, daily storage samples and the forecast
        /// from the latest one. Install admins only; others get `forbidden`.
        "operator.status" => operator_status (Person): Empty => OperatorStatus;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct OperatorStatus {
    /// What `collection.status` returns: availability, analytics lag, watermark and counts.
    pub collection: CollectionStatus,
    /// What `/health/ready` names, in its order. Empty when healthy.
    pub problems: Vec<ReadinessProblem>,
    pub recording: RecordingBudget,
    /// Sizes of the engine files now.
    pub disk: DiskUsage,
    /// The latest run of each maintenance task, retention first. A task that never ran is
    /// missing.
    pub maintenance: Vec<MaintenanceOutcome>,
    /// Daily size samples, oldest first, at most 400 days.
    pub samples: Vec<StorageSample>,
    /// Computed from the latest sample; `None` before the first sample.
    pub forecast: Option<StorageForecast>,
}

/// A problem `/health/ready` reports. The first three stop collection (503).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessProblem {
    /// Free space is below the disk reserve.
    DiskFull,
    /// Durable commits failed in the last minute for another reason.
    StorageUnavailable,
    /// The server is shutting down and admits nothing more.
    ShuttingDown,
    /// Every recent transfer attempt failed: events are stored but not applied.
    TransferFailing,
    /// Transfer works but the oldest waiting record is over 60 s old.
    AnalyticsStale,
    /// Recording refused a part at its cap in the last minute, or uses 90% of it. Ordinary
    /// collection is not affected.
    RecorderBudget,
}

impl ReadinessProblem {
    /// Problems that stop collection; the others leave it ready.
    pub fn blocks_collection(self) -> bool {
        matches!(
            self,
            Self::DiskFull | Self::StorageUnavailable | Self::ShuttingDown
        )
    }
}

/// Bytes on the data volume. Each file counts with its write-ahead log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
pub struct DiskUsage {
    pub control_bytes: i64,
    pub inbox_bytes: i64,
    pub recordings_bytes: i64,
    pub analytics_bytes: i64,
    pub spill_bytes: i64,
    /// Free space on the volume.
    pub free_bytes: i64,
}

impl DiskUsage {
    /// Everything Holocron's files take.
    pub fn used_bytes(&self) -> i64 {
        self.control_bytes
            + self.inbox_bytes
            + self.recordings_bytes
            + self.analytics_bytes
            + self.spill_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct StorageSample {
    /// Unix milliseconds.
    pub taken_at: i64,
    /// Everything Holocron's files took.
    pub used_bytes: i64,
    pub free_bytes: i64,
    /// Eligible events retained, from the daily rollups.
    pub retained_records: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceTask {
    Retention,
    Compaction,
    Backup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceState {
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceOutcome {
    pub task: MaintenanceTask,
    pub state: MaintenanceState,
    /// Unix milliseconds.
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// The failure's fixed code, such as `interrupted`; set when `state` is `failed`.
    pub error_code: Option<String>,
}

/// Whether the disk holds 12 months of data, with what it was computed from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct StorageForecast {
    pub inputs: ForecastInputs,
    pub outcome: ForecastOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ForecastInputs {
    /// The sample the forecast uses, Unix milliseconds. Days count from its UTC date.
    pub sampled_at: i64,
    pub retained_records: i64,
    pub analytics_bytes: i64,
    /// Analytics bytes per retained record; 0 without records.
    pub bytes_per_record: f64,
    /// Median eligible events per day over the last 28 days.
    pub ingest_per_day: i64,
    /// Oldest retained local date `YYYY-MM-DD`; `None` without records.
    pub oldest_day: Option<String>,
    /// Date `YYYY-MM-DD` from which 12 months are retained and disk use levels off.
    pub full_retention_on: Option<String>,
    /// Days from the sample's date until `full_retention_on`, 0 once reached.
    pub days_until_full_retention: Option<i64>,
    /// Control and inbox files, which the forecast keeps at their current size.
    pub sqlite_bytes: i64,
    /// The configured replay cap, which the forecast counts in full.
    pub replay_cap_bytes: i64,
    pub disk_reserve_bytes: i64,
    /// Bytes per record × ingest per day × 365 + SQLite files + replay cap + disk reserve.
    pub steady_state_bytes: i64,
    /// Everything Holocron's files took at the sample.
    pub used_bytes: i64,
    /// Used plus free bytes: what Holocron can take on the volume.
    pub capacity_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForecastOutcome {
    /// Steady state fits: disk use levels off near `size_bytes` on `on` (`YYYY-MM-DD`).
    LevelsOff { size_bytes: i64, on: String },
    /// Steady state does not fit: free space reaches the disk reserve on `on`.
    ReachesReserve { on: String },
    /// Under 14 days of retained data; `days` of it so far.
    NotEnoughHistory { days: i64 },
}
