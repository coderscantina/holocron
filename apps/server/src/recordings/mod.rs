//! Replay and heatmaps (milestone 09): recording policy, the replay store and its cap,
//! playback pages, heatmap cells, and the retention and aggregation work beside them.
//!
//! Replays are kept by capture, the visit of the recorder's session token, never by analytical
//! session; a list or playback links a capture to its current session at read time. The wire
//! side of `/api/record` lives in `collect`.

pub mod frame;
pub mod heatmaps;
pub mod maintenance;
mod operations;
pub mod policy;
pub mod replay;
pub mod store;
pub mod wire;

use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{Days, TimeZone};
use chrono_tz::Tz;
use holocron_storage::Storage;

use store::Store;

/// Default recording cap (briefing: 25 GiB).
pub const DEFAULT_CAP_BYTES: u64 = 25 * 1024 * 1024 * 1024;
/// Raw replay is kept 14 days (0006).
pub const RETENTION_DAYS: u64 = 14;
/// A cap refusal this recent marks recording as refusing in the budget.
const REFUSING_MS: i64 = 60_000;

pub struct Recordings {
    pub store: Store,
    cap_bytes: u64,
    last_refusal: AtomicI64,
}

impl Recordings {
    /// Opens `recordings.sqlite` and the heatmap cell table.
    pub async fn open(data_dir: &Path, storage: &Storage, cap_bytes: u64) -> anyhow::Result<Self> {
        heatmaps::create(storage).await?;
        Ok(Self {
            store: Store::open(data_dir).await?,
            cap_bytes,
            last_refusal: AtomicI64::new(i64::MIN),
        })
    }

    pub fn cap_bytes(&self) -> u64 {
        self.cap_bytes
    }

    /// Saved replays may hold a fifth of the cap: 5 of the default 25 GiB, so at least 20 GiB
    /// always stays for new recording (0006 Saved replays).
    pub fn saved_cap_bytes(&self) -> u64 {
        self.cap_bytes / 5
    }

    pub fn refused_at(&self, now_ms: i64) {
        self.last_refusal.fetch_max(now_ms, Ordering::Relaxed);
    }

    pub fn refusing(&self, now_ms: i64) -> bool {
        self.last_refusal.load(Ordering::Relaxed) > now_ms - REFUSING_MS
    }
}

/// Start of the local date `today - 14 days` in `tz`: unsaved captures first received before
/// it are outside retention (0006 Retention cutoff).
pub fn replay_cutoff_ms(tz: Tz, now_ms: i64) -> i64 {
    let today = tz
        .timestamp_millis_opt(now_ms)
        .single()
        .expect("UTC instants map to one local time")
        .date_naive();
    let date = today
        .checked_sub_days(Days::new(RETENTION_DAYS))
        .expect("date in range");
    let mut local = date.and_hms_opt(0, 0, 0).expect("midnight");
    // A DST gap at midnight starts the day at the first local time that exists.
    loop {
        if let Some(start) = tz.from_local_datetime(&local).earliest() {
            return start.timestamp_millis();
        }
        local += chrono::Duration::minutes(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_cutoff_is_local_midnight_fourteen_days_back() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-03T12:00:00Z")
            .unwrap()
            .timestamp_millis();
        let vienna = replay_cutoff_ms(chrono_tz::Europe::Vienna, now);
        assert_eq!(
            vienna,
            chrono::DateTime::parse_from_rfc3339("2026-09-18T22:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
        let utc = replay_cutoff_ms(chrono_tz::UTC, now);
        assert_eq!(
            utc,
            chrono::DateTime::parse_from_rfc3339("2026-09-19T00:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
    }
}
